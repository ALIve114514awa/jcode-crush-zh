use jcode_lsp::{LspConfig, Manager, Query, ServerConfig, configured_tools};
use jcode_tool_core::{ToolContext, ToolExecutionMode};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};

fn config(project: &Path, options: Value) -> LspConfig {
    let mut options = options;
    options["log_file"] = json!(project.join("events.jsonl"));
    LspConfig {
        enabled: true,
        servers: BTreeMap::from([(
            "fixture".into(),
            ServerConfig {
                command: "node".into(),
                args: vec![format!(
                    "{}/tests/fixtures/server.mjs",
                    env!("CARGO_MANIFEST_DIR")
                )],
                file_types: vec!["ts".into()],
                root_markers: vec!["package.json".into()],
                initialization_options: options,
                settings: json!({"fixture":{"enabled":true}}),
                timeout_secs: 3,
                idle_timeout_secs: 1,
                ..Default::default()
            },
        )]),
    }
}

fn project() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("package.json"), "{}").unwrap();
    directory
}

fn events(project: &Path) -> Vec<Value> {
    std::fs::read_to_string(project.join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn count(project: &Path, name: &str) -> usize {
    events(project)
        .iter()
        .filter(|event| event["event"] == name)
        .count()
}

#[tokio::test]
async fn concurrent_sessions_share_a_server_and_reject_server_edits() {
    let project = project();
    let path = project.path().join("index.ts");
    std::fs::write(&path, "original").unwrap();
    let config = config(project.path(), json!({"apply_edit":true}));
    let manager = Manager::new();
    let (first, second) = tokio::join!(
        manager.query(&config, project.path(), &path, Query::Symbols),
        manager.query(&config, project.path(), &path, Query::Symbols)
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(count(project.path(), "start"), 1);
    assert!(
        events(project.path())
            .iter()
            .any(|event| event["event"] == "edit_response" && event["result"]["applied"] == false)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    manager.shutdown().await;
}

#[tokio::test]
async fn unsupported_capabilities_are_reported_without_querying() {
    let project = project();
    let path = project.path().join("index.ts");
    std::fs::write(&path, "ok").unwrap();
    let config = config(project.path(), json!({"definition":false}));
    let manager = Manager::new();
    let error = manager
        .query(
            &config,
            project.path(),
            &path,
            Query::Definition { line: 1, column: 1 },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("definitionProvider"));
    assert_eq!(count(project.path(), "textDocument/definition"), 0);
    manager.shutdown().await;
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0;
        let success = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        success != 0 && code == 259
    }
}

#[cfg(windows)]
#[tokio::test]
async fn shutdown_kills_the_windows_process_tree() {
    let project = project();
    let path = project.path().join("index.ts");
    std::fs::write(&path, "ok").unwrap();
    let config = config(project.path(), json!({"grandchild":true}));
    let manager = Manager::new();
    manager
        .query(&config, project.path(), &path, Query::Symbols)
        .await
        .unwrap();
    let pid = events(project.path())
        .iter()
        .find(|e| e["event"] == "grandchild")
        .unwrap()["pid"]
        .as_u64()
        .unwrap() as u32;
    assert!(process_alive(pid));
    manager.shutdown().await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while process_alive(pid) {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn lazy_start_reuse_unicode_queries_and_file_synchronization() {
    let project = project();
    let path = project.path().join("中文 空格😀.ts");
    std::fs::write(&path, "中😀value\r\n").unwrap();
    let config = config(project.path(), json!({}));
    let manager = Manager::new();
    assert_eq!(manager.running_servers().await, 0);
    let result = manager
        .query(
            &config,
            project.path(),
            &path,
            Query::Definition { line: 1, column: 3 },
        )
        .await
        .unwrap();
    assert_eq!(
        result["data"][0]["range"]["start"],
        json!({"line":0,"character":3})
    );
    assert_eq!(result["position_encoding"], "utf-16");
    assert!(!result["data"][0]["uri"].as_str().unwrap().contains('\\'));
    let references = manager
        .query(
            &config,
            project.path(),
            &path,
            Query::References {
                line: 1,
                column: 3,
                include_declaration: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(references["data"].as_array().unwrap().len(), 1);
    assert_eq!(count(project.path(), "start"), 1);
    assert_eq!(count(project.path(), "textDocument/didOpen"), 1);
    assert_eq!(count(project.path(), "textDocument/didChange"), 0);
    assert!(
        events(project.path())
            .iter()
            .any(|e| e["event"] == "configuration" && e["settings"][0]["enabled"] == true)
    );
    std::fs::write(&path, "invalid😀\r\n").unwrap();
    let diagnostics = manager
        .query(
            &config,
            project.path(),
            &path,
            Query::Diagnostics { wait_ms: 1000 },
        )
        .await
        .unwrap();
    assert_eq!(diagnostics["document_version"], 2);
    assert_eq!(diagnostics["data"]["fresh"], true);
    assert_eq!(
        diagnostics["data"]["items"][0]["message"],
        "Fixture type error"
    );
    assert_eq!(count(project.path(), "textDocument/didChange"), 1);
    assert_eq!(count(project.path(), "textDocument/didSave"), 1);
    let symbols = manager
        .query(&config, project.path(), &path, Query::Symbols)
        .await
        .unwrap();
    assert_eq!(symbols["data"][0]["name"], "invalid😀");
    manager.shutdown().await;
    assert_eq!(manager.running_servers().await, 0);
}

#[tokio::test]
async fn changes_to_other_open_files_and_deleted_files_are_synchronized() {
    let project = project();
    let first = project.path().join("first.ts");
    let second = project.path().join("second.ts");
    std::fs::write(&first, "first").unwrap();
    std::fs::write(&second, "second").unwrap();
    let config = config(project.path(), json!({"full_sync":true}));
    let manager = Manager::new();
    manager
        .query(&config, project.path(), &first, Query::Symbols)
        .await
        .unwrap();
    manager
        .query(&config, project.path(), &second, Query::Symbols)
        .await
        .unwrap();
    std::fs::write(&first, "updated").unwrap();
    manager
        .query(&config, project.path(), &second, Query::Symbols)
        .await
        .unwrap();
    assert_eq!(count(project.path(), "textDocument/didChange"), 1);
    std::fs::remove_file(&first).unwrap();
    manager
        .query(&config, project.path(), &second, Query::Symbols)
        .await
        .unwrap();
    assert_eq!(count(project.path(), "textDocument/didClose"), 1);
    manager.shutdown().await;
}

#[tokio::test]
async fn changing_another_document_invalidates_cached_diagnostics() {
    let project = project();
    let dependency = project.path().join("dependency.ts");
    let path = project.path().join("index.ts");
    std::fs::write(&dependency, "first").unwrap();
    std::fs::write(&path, "ok").unwrap();
    let config = config(project.path(), json!({}));
    let manager = Manager::new();
    manager
        .query(&config, project.path(), &dependency, Query::Symbols)
        .await
        .unwrap();
    let before = manager
        .query(
            &config,
            project.path(),
            &path,
            Query::Diagnostics { wait_ms: 1000 },
        )
        .await
        .unwrap();
    assert_eq!(before["data"]["received"], true);
    std::fs::write(&dependency, "changed dependency").unwrap();
    // This fixture republishes only the changed file. The client must not
    // claim that the unchanged target's cached diagnostics are still current.
    let after = manager
        .query(
            &config,
            project.path(),
            &path,
            Query::Diagnostics { wait_ms: 100 },
        )
        .await
        .unwrap();
    assert_eq!(after["document_version"], before["document_version"]);
    assert_eq!(after["data"]["received"], false);
    manager.shutdown().await;
}

#[tokio::test]
async fn projects_and_different_server_options_are_isolated() {
    let first = project();
    let second = project();
    for project in [first.path(), second.path()] {
        std::fs::write(project.join("index.ts"), "symbol").unwrap();
    }
    let config = config(first.path(), json!({}));
    let manager = Manager::new();
    let a = manager
        .query(&config, first.path(), Path::new("index.ts"), Query::Symbols)
        .await
        .unwrap();
    let b = manager
        .query(
            &config,
            second.path(),
            Path::new("index.ts"),
            Query::Symbols,
        )
        .await
        .unwrap();
    assert_ne!(a["project_root"], b["project_root"]);
    assert_ne!(a["data"][0]["detail"], b["data"][0]["detail"]);
    assert_eq!(manager.running_servers().await, 2);
    let mut changed = config.clone();
    changed
        .servers
        .get_mut("fixture")
        .unwrap()
        .initialization_options["encoding"] = json!("utf-8");
    manager
        .query(
            &changed,
            first.path(),
            Path::new("index.ts"),
            Query::Symbols,
        )
        .await
        .unwrap();
    assert_eq!(manager.running_servers().await, 3);
    manager.shutdown().await;
}

#[tokio::test]
async fn missing_diagnostics_and_versionless_diagnostics_are_explicit() {
    let project = project();
    std::fs::write(project.path().join("index.ts"), "ok").unwrap();
    let manager = Manager::new();
    let silent = config(project.path(), json!({"publish":false}));
    let result = manager
        .query(
            &silent,
            project.path(),
            Path::new("index.ts"),
            Query::Diagnostics { wait_ms: 30 },
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["received"], false);
    let versionless = config(project.path(), json!({"versionless":true}));
    let result = manager
        .query(
            &versionless,
            project.path(),
            Path::new("index.ts"),
            Query::Diagnostics { wait_ms: 1000 },
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["received"], true);
    assert!(result["data"]["fresh"].is_null());
    let pull = config(project.path(), json!({"pull":true}));
    let result = manager
        .query(
            &pull,
            project.path(),
            Path::new("index.ts"),
            Query::Diagnostics { wait_ms: 0 },
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["mode"], "pull");
    manager.shutdown().await;
}

#[cfg(windows)]
#[tokio::test]
async fn diagnostic_file_uri_aliases_match_the_open_windows_document() {
    let project = project();
    std::fs::write(project.path().join("中文 空格😀.ts"), "invalid").unwrap();
    let manager = Manager::new();
    let config = config(project.path(), json!({"uri_alias":true}));
    let result = manager
        .query(
            &config,
            project.path(),
            Path::new("中文 空格😀.ts"),
            Query::Diagnostics { wait_ms: 1000 },
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["received"], true);
    assert_eq!(result["data"]["fresh"], true);
    assert_eq!(result["data"]["items"][0]["message"], "Fixture type error");
    manager.shutdown().await;
}

#[tokio::test]
async fn crash_fails_fast_and_the_next_query_restarts_the_server() {
    let project = project();
    let path = project.path().join("index.ts");
    std::fs::write(&path, "crash").unwrap();
    let config = config(project.path(), json!({}));
    let manager = Manager::new();
    let started = std::time::Instant::now();
    let error = manager
        .query(&config, project.path(), &path, Query::Symbols)
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("disconnected"));
    assert!(started.elapsed() < Duration::from_secs(2));
    std::fs::write(&path, "recovered").unwrap();
    let result = manager
        .query(&config, project.path(), &path, Query::Symbols)
        .await
        .unwrap();
    assert_eq!(result["data"][0]["name"], "recovered");
    assert_eq!(count(project.path(), "start"), 2);
    manager.shutdown().await;
}

#[tokio::test]
async fn request_timeout_cancels_and_later_queries_work() {
    let project = project();
    let path = project.path().join("index.ts");
    std::fs::write(&path, "hang").unwrap();
    let mut config = config(project.path(), json!({"publish":false}));
    config.servers.get_mut("fixture").unwrap().timeout_secs = 1;
    let manager = Manager::new();
    let error = manager
        .query(&config, project.path(), &path, Query::Symbols)
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("timed out"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(count(project.path(), "$/cancelRequest"), 1);
    std::fs::write(&path, "ok").unwrap();
    manager
        .query(&config, project.path(), &path, Query::Symbols)
        .await
        .unwrap();
    assert_eq!(count(project.path(), "start"), 1);
    manager.shutdown().await;
}

#[tokio::test]
async fn idle_servers_are_reaped() {
    let project = project();
    std::fs::write(project.path().join("index.ts"), "ok").unwrap();
    let manager = Manager::new();
    manager
        .query(
            &config(project.path(), json!({})),
            project.path(),
            Path::new("index.ts"),
            Query::Symbols,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        while manager.running_servers().await != 0 || count(project.path(), "shutdown") == 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(count(project.path(), "shutdown"), 1);
}

#[tokio::test]
async fn disabled_missing_servers_and_outside_files_fail_clearly() {
    let project = project();
    let outside = self::project();
    std::fs::write(project.path().join("index.ts"), "ok").unwrap();
    std::fs::write(outside.path().join("outside.ts"), "ok").unwrap();
    let manager = Manager::new();
    assert!(
        manager
            .query(
                &LspConfig::default(),
                project.path(),
                Path::new("index.ts"),
                Query::Symbols
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("disabled")
    );
    let mut config = config(project.path(), json!({}));
    assert!(
        manager
            .query(
                &config,
                project.path(),
                &outside.path().join("outside.ts"),
                Query::Symbols
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("inside")
    );
    config.servers.get_mut("fixture").unwrap().command =
        "jcode-no-such-language-server-948723".into();
    assert!(
        manager
            .query(
                &config,
                project.path(),
                Path::new("index.ts"),
                Query::Symbols
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("Cannot start")
    );
    assert_eq!(manager.running_servers().await, 0);
    assert!(configured_tools(LspConfig::default()).is_empty());
}

#[tokio::test]
async fn registry_tools_use_host_interrupts_and_remain_usable() {
    let project = project();
    let path = project.path().join("index.ts");
    std::fs::write(&path, "hang").unwrap();
    let tools = configured_tools(config(project.path(), json!({"publish":false})));
    assert_eq!(tools.len(), 4);
    for tool in &tools {
        assert!(
            tool.to_definition().input_schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("intent"))
        );
    }
    let symbols = tools
        .iter()
        .find(|tool| tool.name() == "lsp_symbols")
        .unwrap();
    let context = ToolContext {
        session_id: "test".into(),
        message_id: "message".into(),
        tool_call_id: "call".into(),
        working_dir: Some(project.path().into()),
        stdin_request_tx: None,
        graceful_shutdown_signal: Some(Default::default()),
        execution_mode: ToolExecutionMode::Direct,
    };
    let signal = context.graceful_shutdown_signal.clone().unwrap();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        signal.fire();
    });
    let error = symbols
        .execute(json!({"file_path":"index.ts"}), context.clone())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    context.graceful_shutdown_signal.as_ref().unwrap().reset();
    std::fs::write(&path, "resumed").unwrap();
    let output = symbols
        .execute(json!({"file_path":"index.ts"}), context)
        .await
        .unwrap();
    assert!(output.output.contains("resumed"));
    // Keep the fixture directory available until the shared manager reaps it.
    tokio::time::sleep(Duration::from_millis(2200)).await;
}
