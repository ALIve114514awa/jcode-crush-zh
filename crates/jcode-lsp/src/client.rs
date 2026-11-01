use crate::{
    ServerConfig, framing,
    position::{self, Encoding},
    process::ProcessTree,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::BufReader,
    process::{Child, Command},
    sync::{Mutex, mpsc, oneshot, watch},
    task::JoinHandle,
};

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_OPEN_DOCUMENTS: usize = 64;
type Reply = std::result::Result<Value, String>;

struct Inner {
    writer: mpsc::Sender<Value>,
    pending: StdMutex<HashMap<u64, oneshot::Sender<Reply>>>,
    next_id: AtomicU64,
    closed: AtomicBool,
    versions: StdMutex<HashMap<String, i64>>,
    diagnostics: StdMutex<HashMap<String, Value>>,
    diagnostics_changed: watch::Sender<u64>,
    settings: Value,
    root_uri: String,
    root_name: String,
}

impl Inner {
    fn track_version(&self, uri: &str, version: i64) {
        let mut versions = self.versions.lock().unwrap_or_else(|p| p.into_inner());
        let mut diagnostics = self.diagnostics.lock().unwrap_or_else(|p| p.into_inner());
        versions.insert(position::document_key(uri), version);
        // Changing a dependency can invalidate diagnostics in other open files
        // even when those files keep the same document version.
        diagnostics.clear();
    }

    fn forget_document(&self, uri: &str) {
        let mut versions = self.versions.lock().unwrap_or_else(|p| p.into_inner());
        let mut diagnostics = self.diagnostics.lock().unwrap_or_else(|p| p.into_inner());
        versions.remove(&position::document_key(uri));
        diagnostics.clear();
    }

    fn close(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        self.closed.store(true, Ordering::SeqCst);
        for (_, reply) in pending.drain() {
            let _ = reply.send(Err("Language server disconnected".into()));
        }
        self.diagnostics_changed
            .send_modify(|revision| *revision += 1);
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            bail!("Language server disconnected");
        }
        self.writer
            .send(json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await
            .context("LSP writer disconnected")
    }

    async fn receive(&self, message: Value) -> Result<()> {
        if let Some(method) = message["method"].as_str() {
            if let Some(id) = message.get("id") {
                let response = match method {
                    "workspace/configuration" => {
                        let items = message["params"]["items"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        let values: Vec<Value> = items
                            .iter()
                            .map(|item| {
                                let section = item["section"].as_str().unwrap_or("");
                                if section.is_empty() {
                                    self.settings.clone()
                                } else {
                                    section
                                        .split('.')
                                        .fold(&self.settings, |value, key| &value[key])
                                        .clone()
                                }
                            })
                            .collect();
                        json!({"jsonrpc":"2.0","id":id,"result":values})
                    }
                    "workspace/workspaceFolders" => {
                        json!({"jsonrpc":"2.0","id":id,"result":[{"uri":self.root_uri,"name":self.root_name}]})
                    }
                    "workspace/applyEdit" => {
                        json!({"jsonrpc":"2.0","id":id,"result":{"applied":false,"failureReason":"JCode LSP queries do not apply workspace edits"}})
                    }
                    "window/workDoneProgress/create" => {
                        json!({"jsonrpc":"2.0","id":id,"result":null})
                    }
                    _ => {
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Client method not supported"}})
                    }
                };
                self.writer
                    .send(response)
                    .await
                    .context("LSP writer disconnected")?;
            } else if method == "textDocument/publishDiagnostics" {
                let params = &message["params"];
                if let Some(uri) = params["uri"].as_str() {
                    let document_key = position::document_key(uri);
                    let versions = self.versions.lock().unwrap_or_else(|p| p.into_inner());
                    let expected = versions.get(&document_key).copied();
                    let published = params["version"].as_i64();
                    // Versionless push diagnostics cannot prove freshness. The
                    // result keeps that distinction instead of claiming success.
                    if expected.is_some() && (published.is_none() || published == expected) {
                        self.diagnostics
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .insert(
                                document_key,
                                json!({
                                    "items":params["diagnostics"], "version":published,
                                    "received":true, "fresh":published.map(|v| Some(v) == expected)
                                }),
                            );
                        self.diagnostics_changed
                            .send_modify(|revision| *revision += 1);
                    }
                }
            }
        } else if let Some(id) = message["id"].as_u64() {
            let reply = self
                .pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id);
            if let Some(reply) = reply {
                let value = if let Some(error) = message.get("error") {
                    Err(format!(
                        "LSP error {}: {}",
                        error["code"],
                        error["message"].as_str().unwrap_or("unknown error")
                    ))
                } else {
                    Ok(message.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = reply.send(value);
            }
        }
        Ok(())
    }
}

/// Dropping a query (including host cancellation) removes its waiter and
/// requests cancellation without leaving entries behind in the pending map.
struct PendingRequest {
    inner: Arc<Inner>,
    id: u64,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        if self
            .inner
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id)
            .is_some()
        {
            let _ = self.inner.writer.try_send(
                json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":self.id}}),
            );
        }
    }
}

struct Document {
    path: PathBuf,
    text: String,
    version: i64,
}

pub(crate) struct Client {
    inner: Arc<Inner>,
    child: Mutex<Child>,
    _tree: ProcessTree,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
    pub(crate) capabilities: Value,
    pub(crate) encoding: Encoding,
    sync_kind: u64,
    open_close: bool,
    save: Value,
    timeout: Duration,
    pub(crate) idle_timeout: Duration,
    last_used: StdMutex<Instant>,
    active: AtomicUsize,
    pub(crate) query_lock: Mutex<()>,
    documents: Mutex<BTreeMap<String, Document>>,
    language_id: String,
}

impl Client {
    pub(crate) async fn start(name: &str, config: &ServerConfig, root: &Path) -> Result<Self> {
        if config.command.is_empty() {
            bail!("LSP server '{name}' has no command configured");
        }
        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW.
        // Provider credentials are inherited only when explicitly configured.
        for (key, _) in std::env::vars_os() {
            let upper = key.to_string_lossy().to_ascii_uppercase();
            if upper.ends_with("_API_KEY")
                || upper.ends_with("_ACCESS_TOKEN")
                || upper.ends_with("_AUTH_TOKEN")
                || matches!(
                    upper.as_str(),
                    "AWS_ACCESS_KEY_ID"
                        | "AWS_SECRET_ACCESS_KEY"
                        | "AWS_SESSION_TOKEN"
                        | "AZURE_CLIENT_SECRET"
                        | "GOOGLE_APPLICATION_CREDENTIALS"
                )
            {
                command.env_remove(key);
            }
        }
        command.envs(&config.env);
        let mut child = command.spawn().with_context(|| {
            format!("Cannot start LSP server '{name}'; check its command and installation")
        })?;
        let tree = ProcessTree::attach(child.id().context("LSP process has no PID")?)?;
        let mut stdin = child.stdin.take().context("LSP stdin unavailable")?;
        let stdout = child.stdout.take().context("LSP stdout unavailable")?;
        let (writer_tx, mut writer_rx) = mpsc::channel(64);
        let (changed, _) = watch::channel(0);
        let inner = Arc::new(Inner {
            writer: writer_tx,
            pending: StdMutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            versions: StdMutex::new(HashMap::new()),
            diagnostics: StdMutex::new(HashMap::new()),
            diagnostics_changed: changed,
            settings: config.settings.clone(),
            root_uri: position::uri(root)?,
            root_name: root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
        });
        let writer_inner = inner.clone();
        let writer = tokio::spawn(async move {
            while let Some(message) = writer_rx.recv().await {
                if framing::write(&mut stdin, &message).await.is_err() {
                    break;
                }
            }
            writer_inner.close();
        });
        let reader_inner = inner.clone();
        let reader = tokio::spawn(async move {
            let mut stdout = BufReader::new(stdout);
            while let Ok(message) = framing::read(&mut stdout).await {
                if reader_inner.receive(message).await.is_err() {
                    break;
                }
            }
            reader_inner.close();
        });
        let mut client = Self {
            inner,
            child: Mutex::new(child),
            _tree: tree,
            reader,
            writer,
            capabilities: Value::Null,
            encoding: Encoding::Utf16,
            sync_kind: 0,
            open_close: false,
            save: Value::Null,
            timeout: Duration::from_secs(config.timeout_secs.max(1)),
            idle_timeout: Duration::from_secs(config.idle_timeout_secs.max(1)),
            last_used: StdMutex::new(Instant::now()),
            active: AtomicUsize::new(0),
            query_lock: Mutex::new(()),
            documents: Mutex::new(BTreeMap::new()),
            language_id: config.language_id.clone(),
        };
        let result = client.request("initialize", json!({
            "processId":std::process::id(),"clientInfo":{"name":"jcode","version":"0.1.0"},
            "rootUri":client.inner.root_uri,
            "workspaceFolders":[{"uri":client.inner.root_uri,"name":client.inner.root_name}],
            "initializationOptions":config.initialization_options,
            "capabilities":{
                "general":{"positionEncodings":["utf-16","utf-8","utf-32"]},
                "workspace":{"configuration":true,"workspaceFolders":true,"applyEdit":false},
                "textDocument":{
                    "synchronization":{"dynamicRegistration":false,"didSave":true},
                    "definition":{"dynamicRegistration":false,"linkSupport":true},
                    "references":{"dynamicRegistration":false},
                    "documentSymbol":{"dynamicRegistration":false,"hierarchicalDocumentSymbolSupport":true},
                    "publishDiagnostics":{"versionSupport":true},
                    "diagnostic":{"dynamicRegistration":false}
                }
            }
        })).await.with_context(|| format!("LSP server '{name}' initialization failed"))?;
        client.capabilities = result["capabilities"].clone();
        client.encoding = Encoding::parse(client.capabilities["positionEncoding"].as_str())?;
        let sync = &client.capabilities["textDocumentSync"];
        client.sync_kind = sync
            .as_u64()
            .or_else(|| sync["change"].as_u64())
            .unwrap_or(0);
        client.open_close = sync.is_number() || sync["openClose"].as_bool().unwrap_or(false);
        client.save = sync["save"].clone();
        client.inner.notify("initialized", json!({})).await?;
        client
            .inner
            .notify(
                "workspace/didChangeConfiguration",
                json!({"settings":config.settings}),
            )
            .await?;
        Ok(client)
    }

    pub(crate) async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.inner.pending.lock().unwrap_or_else(|p| p.into_inner());
            if self.closed() {
                bail!("Language server disconnected");
            }
            pending.insert(id, tx);
        }
        let _guard = PendingRequest {
            inner: self.inner.clone(),
            id,
        };
        tokio::time::timeout(self.timeout, async {
            self.inner
                .writer
                .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .await
                .context("LSP writer disconnected")?;
            match rx.await.context("LSP response channel disconnected")? {
                Ok(value) => Ok(value),
                Err(error) => bail!("{error}"),
            }
        })
        .await
        .with_context(|| format!("LSP {method} timed out after {}s", self.timeout.as_secs()))?
    }

    pub(crate) fn closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    pub(crate) fn activity(self: &Arc<Self>) -> Activity {
        self.active.fetch_add(1, Ordering::SeqCst);
        *self.last_used.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
        Activity(self.clone())
    }

    pub(crate) fn idle(&self) -> bool {
        self.active.load(Ordering::SeqCst) == 0
            && self
                .last_used
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .elapsed()
                >= self.idle_timeout
    }

    pub(crate) async fn shutdown(&self) {
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            self.request("shutdown", Value::Null),
        )
        .await;
        let _ = self.inner.notify("exit", Value::Null).await;
        self.inner.close();
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    async fn close_document(
        &self,
        uri: &str,
        documents: &mut BTreeMap<String, Document>,
    ) -> Result<()> {
        if self.open_close {
            self.inner
                .notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
                .await?;
        }
        // Commit only after enqueueing the notification, so cancellation
        // while the writer is backpressured cannot forget an open document.
        documents.remove(uri);
        self.inner.forget_document(uri);
        Ok(())
    }

    /// Refresh all previously queried files as well as the target. This makes
    /// cross-file queries see edits made by the host's existing editing tools.
    pub(crate) async fn sync(&self, target: &Path) -> Result<(String, i64)> {
        if !matches!(self.sync_kind, 1 | 2) {
            bail!("Language server does not support document synchronization");
        }
        let target_uri = position::uri(target)?;
        let mut documents = self.documents.lock().await;
        if !documents.contains_key(&target_uri) && documents.len() >= MAX_OPEN_DOCUMENTS {
            let oldest = documents.keys().next().unwrap().clone();
            self.close_document(&oldest, &mut documents).await?;
        }
        let mut paths: Vec<PathBuf> = documents
            .values()
            .map(|document| document.path.clone())
            .collect();
        if !documents.contains_key(&target_uri) {
            paths.push(target.into());
        }
        for path in paths {
            let file_uri = position::uri(&path)?;
            let text = match read_source(&path).await {
                Ok(text) => text,
                Err(error) if path == target => return Err(error),
                Err(_) => {
                    self.close_document(&file_uri, &mut documents).await?;
                    continue;
                }
            };
            if let Some(document) = documents.get_mut(&file_uri) {
                if document.text == text {
                    continue;
                }
                let next_version = document.version + 1;
                self.inner.track_version(&file_uri, next_version);
                let changes = if self.sync_kind == 2 {
                    json!([{"range":{"start":{"line":0,"character":0},"end":self.encoding.end(&document.text)},"text":text}])
                } else {
                    json!([{"text":text}])
                };
                self.inner.notify("textDocument/didChange", json!({"textDocument":{"uri":file_uri,"version":next_version},"contentChanges":changes})).await?;
                document.text = text;
                document.version = next_version;
                if self.save == Value::Bool(true) || self.save.is_object() {
                    let mut params = json!({"textDocument":{"uri":file_uri}});
                    if self.save["includeText"].as_bool().unwrap_or(false) {
                        params["text"] = json!(document.text);
                    }
                    self.inner.notify("textDocument/didSave", params).await?;
                }
            } else {
                self.inner.track_version(&file_uri, 1);
                if self.open_close {
                    let extension = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let language = if self.language_id.is_empty() {
                        position::language_id(&extension)
                    } else {
                        &self.language_id
                    };
                    self.inner.notify("textDocument/didOpen", json!({"textDocument":{"uri":file_uri,"languageId":language,"version":1,"text":text}})).await?;
                }
                documents.insert(
                    file_uri,
                    Document {
                        path,
                        text,
                        version: 1,
                    },
                );
            }
        }
        let target = documents
            .get(&target_uri)
            .context("Target document was not synchronized")?;
        Ok((target.text.clone(), target.version))
    }

    pub(crate) async fn diagnostics(&self, uri: &str, wait: Duration) -> Result<Value> {
        if capability(&self.capabilities, "diagnosticProvider") {
            let result = self
                .request(
                    "textDocument/diagnostic",
                    json!({"textDocument":{"uri":uri}}),
                )
                .await?;
            return Ok(json!({"items":result["items"],"received":true,"fresh":true,"mode":"pull"}));
        }
        let document_key = position::document_key(uri);
        let mut changed = self.inner.diagnostics_changed.subscribe();
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(result) = self
                .inner
                .diagnostics
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&document_key)
                .cloned()
            {
                return Ok(result);
            }
            if self.closed() {
                bail!("Language server disconnected while waiting for diagnostics");
            }
            if !matches!(
                tokio::time::timeout_at(deadline, changed.changed()).await,
                Ok(Ok(()))
            ) {
                return Ok(
                    json!({"items":[],"received":false,"fresh":false,"mode":"push","status":"No diagnostics publication arrived within the wait window"}),
                );
            }
        }
    }
}

pub(crate) fn capability(capabilities: &Value, name: &str) -> bool {
    matches!(
        capabilities.get(name),
        Some(Value::Bool(true) | Value::Object(_))
    )
}

pub(crate) async fn read_source(path: &Path) -> Result<String> {
    if tokio::fs::metadata(path).await?.len() > MAX_FILE_BYTES {
        bail!("LSP query files must be at most 4 MiB");
    }
    tokio::fs::read_to_string(path)
        .await
        .context("LSP query requires a readable UTF-8 file")
}

pub(crate) struct Activity(Arc<Client>);
impl Drop for Activity {
    fn drop(&mut self) {
        *self.0.last_used.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.inner.close();
        self.reader.abort();
        self.writer.abort();
    }
}
