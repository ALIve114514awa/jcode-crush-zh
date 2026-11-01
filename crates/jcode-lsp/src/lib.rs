//! On-demand, read-only LSP queries for JCode's existing tool registry.
mod client;
mod config;
mod framing;
mod position;
mod process;
mod tool;

pub use config::{LspConfig, ServerConfig};
pub use tool::configured_tools;

use anyhow::{Context, Result, bail};
use client::{Client, capability};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, OnceCell};

#[derive(Clone, Debug)]
pub enum Query {
    Definition {
        line: usize,
        column: usize,
    },
    References {
        line: usize,
        column: usize,
        include_declaration: bool,
    },
    Symbols,
    Diagnostics {
        wait_ms: u64,
    },
}

// The full configuration participates in identity so projects or sessions
// using different initialization options never share a server accidentally.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    root: PathBuf,
    name: String,
    config: String,
}
type Cell = Arc<OnceCell<Arc<Client>>>;

#[derive(Default)]
struct State {
    clients: Mutex<BTreeMap<Key, Cell>>,
    pruning: AtomicBool,
}

#[derive(Clone, Default)]
pub struct Manager {
    state: Arc<State>,
}

impl Manager {
    pub fn new() -> Self {
        Self::default()
    }

    fn start_pruning(&self) {
        if self.state.pruning.swap(true, Ordering::SeqCst) {
            return;
        }
        let state = Arc::downgrade(&self.state);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(state) = state.upgrade() else {
                    break;
                };
                let expired = {
                    let mut clients = state.clients.lock().await;
                    let keys: Vec<Key> = clients
                        .iter()
                        .filter(|(_, cell)| {
                            cell.get()
                                .is_some_and(|client| client.closed() || client.idle())
                        })
                        .map(|(key, _)| key.clone())
                        .collect();
                    keys.into_iter()
                        .filter_map(|key| clients.remove(&key))
                        .filter_map(|cell| cell.get().cloned())
                        .collect::<Vec<_>>()
                };
                for client in expired {
                    client.shutdown().await;
                }
            }
        });
    }

    pub async fn query(
        &self,
        config: &LspConfig,
        workspace: &Path,
        file: &Path,
        query: Query,
    ) -> Result<Value> {
        if !config.enabled {
            bail!("LSP is disabled; enable [lsp] in config.toml");
        }
        let workspace = tokio::fs::canonicalize(workspace)
            .await
            .context("LSP working directory does not exist")?;
        let file = if file.is_absolute() {
            file.to_path_buf()
        } else {
            workspace.join(file)
        };
        let file = tokio::fs::canonicalize(file)
            .await
            .context("LSP query file does not exist")?;
        if !file.starts_with(&workspace) {
            bail!("LSP query file must be inside the working directory");
        }
        let extension = file
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let (name, server) = config
            .servers
            .iter()
            .find(|(_, server)| {
                !server.disabled
                    && server
                        .file_types
                        .iter()
                        .any(|kind| kind.trim_start_matches('.').eq_ignore_ascii_case(extension))
            })
            .with_context(|| format!("No configured LSP server handles '.{extension}' files"))?;
        let root = project_root(&workspace, &file, &server.root_markers)?;
        let key = Key {
            root: root.clone(),
            name: name.clone(),
            config: serde_json::to_string(server)?,
        };
        let mut cell = {
            let mut clients = self.state.clients.lock().await;
            if clients
                .get(&key)
                .and_then(|cell| cell.get())
                .is_some_and(|client| client.closed())
            {
                clients.remove(&key);
            }
            clients.entry(key.clone()).or_default().clone()
        };
        self.start_pruning();
        let result = tokio::time::timeout(Duration::from_secs(server.timeout_secs.max(1)), async {
            let (client, _activity) = loop {
                let client = cell.get_or_try_init(|| async { Client::start(name, server, &root).await.map(Arc::new) }).await?.clone();
                let mut clients = self.state.clients.lock().await;
                if clients.get(&key).is_some_and(|current| Arc::ptr_eq(current, &cell)) {
                    // Claim activity under the same lock used by idle pruning.
                    // A server removed while initialization was awaited must
                    // be replaced before this query uses it.
                    let activity = client.activity();
                    break (client, activity);
                }
                cell = clients.entry(key.clone()).or_default().clone();
            };
            let _query_lock = client.query_lock.lock().await;
            let required = match query {
                Query::Definition { .. } => Some("definitionProvider"),
                Query::References { .. } => Some("referencesProvider"),
                Query::Symbols => Some("documentSymbolProvider"),
                Query::Diagnostics { .. } => None,
            };
            if let Some(required) = required && !capability(&client.capabilities, required) {
                bail!("LSP server '{name}' does not advertise {required}");
            }
            let (text, version) = client.sync(&file).await?;
            let uri = position::uri(&file)?;
            let document = json!({"uri":uri});
            let data = match query {
                Query::Definition { line, column } => client.request("textDocument/definition", json!({"textDocument":document,"position":client.encoding.position(&text,line,column)?})).await?,
                Query::References { line, column, include_declaration } => client.request("textDocument/references", json!({"textDocument":document,"position":client.encoding.position(&text,line,column)?,"context":{"includeDeclaration":include_declaration}})).await?,
                Query::Symbols => client.request("textDocument/documentSymbol", json!({"textDocument":document})).await?,
                Query::Diagnostics { wait_ms } => client.diagnostics(&uri, Duration::from_millis(wait_ms.min(5000))).await?,
            };
            Ok::<_, anyhow::Error>(json!({
                "file_path":file,"project_root":root,"server":name,"document_version":version,
                "position_encoding":client.encoding.name(),"line_base":0,"character_base":0,"data":data
            }))
        }).await.with_context(|| format!("LSP query for '{name}' timed out after {}s", server.timeout_secs.max(1)))?;
        if cell.get().is_some_and(|client| client.closed()) {
            let mut clients = self.state.clients.lock().await;
            if clients
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, &cell))
            {
                clients.remove(&key);
            }
        }
        result
    }

    /// Explicit cleanup is useful for standalone clients and test harnesses.
    pub async fn shutdown(&self) {
        let cells = std::mem::take(&mut *self.state.clients.lock().await);
        for cell in cells.into_values() {
            if let Some(client) = cell.get() {
                client.shutdown().await;
            }
        }
    }

    pub async fn running_servers(&self) -> usize {
        self.state
            .clients
            .lock()
            .await
            .values()
            .filter(|cell| cell.get().is_some_and(|client| !client.closed()))
            .count()
    }
}

fn project_root(workspace: &Path, file: &Path, markers: &[String]) -> Result<PathBuf> {
    if markers.iter().any(|marker| {
        Path::new(marker).components().count() != 1 || marker == "." || marker == ".."
    }) {
        bail!("LSP root markers must be individual file or directory names");
    }
    let mut directory = file.parent().context("LSP file has no parent directory")?;
    loop {
        if markers.iter().any(|marker| directory.join(marker).exists()) {
            return Ok(directory.into());
        }
        if directory == workspace {
            return Ok(workspace.into());
        }
        directory = directory
            .parent()
            .context("Could not resolve LSP project root")?;
    }
}
