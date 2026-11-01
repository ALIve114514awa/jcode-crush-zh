use anyhow::{Context, Result, ensure};
use clap::Parser;
use jcode::id::new_id;
use jcode::message::{Message, ToolDefinition};
use jcode::provider::{EventStream, Provider};
use jcode::tool::{Registry, ToolContext, ToolExecutionMode};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

#[path = "harness/fusion_agent.rs"]
mod fusion_agent;

#[derive(Parser)]
#[command(name = "jcode-harness")]
#[command(about = "Run a deterministic tool harness smoke test")]
struct Args {
    /// Use an explicit working directory (defaults to a temp folder).
    #[arg(long)]
    cwd: Option<String>,

    /// Include network-backed tools (webfetch/websearch).
    #[arg(long)]
    include_network: bool,

    /// Verify the configured LSP and local remote-MCP fixtures through Registry.
    #[arg(long)]
    fusion: bool,
}

struct NoopProvider;

#[async_trait::async_trait]
impl Provider for NoopProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        anyhow::bail!("Noop provider - tool harness does not invoke models.")
    }

    fn name(&self) -> &str {
        "noop"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(NoopProvider)
    }

    fn available_models_display(&self) -> Vec<String> {
        vec![]
    }

    async fn prefetch_models(&self) -> Result<()> {
        Ok(())
    }
}

struct ToolCase {
    name: &'static str,
    input: serde_json::Value,
    label: &'static str,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let workspace = if let Some(cwd) = args.cwd {
        PathBuf::from(cwd)
    } else {
        create_temp_workspace()?
    };

    std::fs::create_dir_all(&workspace)?;
    std::env::set_current_dir(&workspace)?;
    eprintln!("Harness workspace: {}", workspace.display());

    let provider: Arc<dyn Provider> = Arc::new(NoopProvider);
    let registry = Registry::new(provider).await;

    let session_id = new_id("harness");
    let base_ctx = ToolContext {
        session_id: session_id.clone(),
        message_id: session_id.clone(),
        tool_call_id: String::new(),
        working_dir: Some(workspace.clone()),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    };

    if args.fusion {
        return verify_fusion(&registry, &base_ctx).await;
    }

    let mut cases = Vec::new();
    cases.push(ToolCase {
        name: "write",
        label: "write sample.txt",
        input: json!({"file_path": "sample.txt", "content": "alpha\nbeta\n"}),
    });
    cases.push(ToolCase {
        name: "read",
        label: "read sample.txt",
        input: json!({"file_path": "sample.txt"}),
    });
    cases.push(ToolCase {
        name: "edit",
        label: "edit sample.txt (alpha -> alpha1)",
        input: json!({"file_path": "sample.txt", "old_string": "alpha", "new_string": "alpha1"}),
    });
    cases.push(ToolCase {
        name: "multiedit",
        label: "multiedit sample.txt",
        input: json!({
            "file_path": "sample.txt",
            "edits": [
                {"old_string": "alpha1", "new_string": "alpha2"},
                {"old_string": "beta", "new_string": "beta1"}
            ]
        }),
    });
    cases.push(ToolCase {
        name: "patch",
        label: "patch sample.txt",
        input: json!({"patch_text": "--- a/sample.txt\n+++ b/sample.txt\n@@ -1,2 +1,3 @@\n alpha2\n beta1\n+gamma\n"}),
    });
    cases.push(ToolCase {
        name: "apply_patch",
        label: "apply_patch add file",
        input: json!({"patch_text": "*** Begin Patch\n*** Add File: added.txt\n+added\n*** End Patch\n"}),
    });
    cases.push(ToolCase {
        name: "ls",
        label: "ls .",
        input: json!({"path": "."}),
    });
    cases.push(ToolCase {
        name: "bash",
        label: "bash pwd",
        input: json!({"command": "pwd"}),
    });
    cases.push(ToolCase {
        name: "invalid",
        label: "invalid tool call",
        input: json!({"tool": "unknown", "error": "missing required field"}),
    });
    cases.push(ToolCase {
        name: "todo",
        label: "todo write",
        input: json!({"todos": [{"content": "harness task", "status": "pending", "priority": "low", "id": "1"}]}),
    });
    cases.push(ToolCase {
        name: "todo",
        label: "todo read",
        input: json!({}),
    });
    cases.push(ToolCase {
        name: "batch",
        label: "batch ls + read",
        input: json!({
            "tool_calls": [
                {"tool": "ls", "parameters": {"path": "."}},
                {"tool": "read", "parameters": {"file_path": "sample.txt"}}
            ]
        }),
    });

    if args.include_network {
        cases.push(ToolCase {
            name: "webfetch",
            label: "webfetch example.com",
            input: json!({"url": "https://example.com", "format": "text"}),
        });
        cases.push(ToolCase {
            name: "websearch",
            label: "websearch rust async",
            input: json!({"query": "rust async await"}),
        });
    }

    for (idx, case) in cases.iter().enumerate() {
        let ctx = ToolContext {
            tool_call_id: format!("harness-{}", idx + 1),
            ..base_ctx.clone()
        };
        println!("\n== {} ({}) ==", case.name, case.label);
        match registry.execute(case.name, case.input.clone(), ctx).await {
            Ok(output) => {
                if let Some(title) = output.title {
                    println!("[title] {}", title);
                }
                println!("{}", output.output);
            }
            Err(err) => {
                println!("[error] {}", err);
            }
        }
    }

    Ok(())
}

async fn verify_fusion(registry: &Registry, ctx: &ToolContext) -> Result<()> {
    use serde_json::Value;
    use std::time::Duration;
    println!("{}", json!({"stage":"baseline","pid":std::process::id()}));
    tokio::time::sleep(Duration::from_secs(1)).await;
    println!("{}", json!({"stage":"stdio_starting"}));
    verify_stdio_recovery().await?;
    fusion_agent::verify().await?;
    async fn query(
        registry: &Registry,
        ctx: &ToolContext,
        name: &str,
        input: Value,
    ) -> Result<Value> {
        let output = registry.execute(name, input, ctx.clone()).await?;
        println!("{}", json!({"tool": name, "output": output.output}));
        Ok(serde_json::from_str(&output.output)?)
    }
    async fn diagnostics(
        registry: &Registry,
        ctx: &ToolContext,
        file: &str,
        expect_error: bool,
    ) -> Result<Value> {
        // TypeScript can publish an empty syntax result before its semantic
        // result. Assert eventual state within a bound, not publication order.
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let current = query(
                    registry,
                    ctx,
                    "lsp_diagnostics",
                    json!({"file_path":file,"wait_ms":1000}),
                )
                .await?;
                let matches = current["data"]["received"] == true
                    && current["data"]["items"].as_array().is_some_and(|items| {
                        if expect_error {
                            items.iter().any(|d| d["code"] == 2322)
                        } else {
                            items.iter().all(|d| d["severity"] != 1)
                        }
                    });
                if matches {
                    return Ok::<Value, anyhow::Error>(current);
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        })
        .await
        .context("Timed out waiting for the expected asynchronous diagnostics")?
    }
    let file = "调用 空格😀.ts";
    println!("{}", json!({"stage":"lsp_starting"}));
    let source = "import { greet } from './defs.js';\r\nconst 文本 = '中文😀'; greet(文本);\r\nconst broken: number = 'wrong';\r\n";
    registry.execute("write", json!({"file_path":"defs.ts","content":"export function greet(name: string): string { return 'Hello ' + name; }\r\n"}), ctx.clone()).await?;
    registry
        .execute(
            "write",
            json!({"file_path":file,"content":source}),
            ctx.clone(),
        )
        .await?;
    let read = registry
        .execute("read", json!({"file_path":file}), ctx.clone())
        .await?;
    ensure!(read.output.contains("broken"), "Host read tool failed");
    let search = registry
        .execute(
            "agentgrep",
            json!({"query":"greet","path":".","mode":"grep"}),
            ctx.clone(),
        )
        .await?;
    ensure!(search.output.contains("defs.ts"), "Host search tool failed");
    let command = registry
        .execute(
            "bash",
            json!({"command":"echo fusion-command-ok"}),
            ctx.clone(),
        )
        .await?;
    ensure!(
        command.output.contains("fusion-command-ok"),
        "Host command tool failed"
    );
    let symbols = query(registry, ctx, "lsp_symbols", json!({"file_path":"defs.ts"})).await?;
    ensure!(
        symbols["data"]
            .as_array()
            .is_some_and(|items| items.iter().any(|s| s["name"] == "greet")),
        "Missing symbol"
    );
    let line = source.lines().nth(1).unwrap();
    let column = line[..line.find("greet").unwrap()].chars().count() + 1;
    let definition = query(
        registry,
        ctx,
        "lsp_definition",
        json!({"file_path":file,"line":2,"column":column}),
    )
    .await?;
    ensure!(
        definition["data"].to_string().contains("defs.ts"),
        "Definition failed"
    );
    let references = query(
        registry,
        ctx,
        "lsp_references",
        json!({"file_path":file,"line":2,"column":column}),
    )
    .await?;
    ensure!(
        references["data"]
            .as_array()
            .is_some_and(|items| items.len() >= 3),
        "References failed"
    );
    let before = diagnostics(registry, ctx, file, true).await?;
    ensure!(
        before["data"]["received"] == true
            && before["data"]["items"]
                .as_array()
                .is_some_and(|items| items.iter().any(|d| d["code"] == 2322)),
        "Expected type error"
    );
    registry
        .execute(
            "edit",
            json!({"file_path":file,"old_string":"'wrong'","new_string":"42"}),
            ctx.clone(),
        )
        .await?;
    let after = diagnostics(registry, ctx, file, false).await?;
    ensure!(
        after["data"]["received"] == true
            && after["document_version"].as_i64() > before["document_version"].as_i64()
            && after["data"]["items"]
                .as_array()
                .is_some_and(|items| items.iter().all(|d| d["severity"] != 1)),
        "Diagnostics did not update after the host edit tool"
    );

    println!("{}", json!({"stage":"lsp_ready"}));
    tokio::time::sleep(Duration::from_secs(1)).await;
    println!("{}", json!({"stage":"remote_mcp_starting"}));
    verify_remote_recovery(ctx).await?;
    verify_mcp_sessions(ctx).await?;
    registry
        .register_mcp_tools(None, None, Some(ctx.session_id.clone()))
        .await;
    let expected = ["mcp__fixture_http__echo", "mcp__fixture_sse__echo"];
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let names = registry.tool_names().await;
            if expected.iter().all(|name| names.iter().any(|n| n == name)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await?;
    for name in expected {
        let output = registry
            .execute(name, json!({"text":"中文 😀 Registry"}), ctx.clone())
            .await?;
        ensure!(
            output.output.contains("中文 😀 Registry"),
            "Remote MCP output mismatch: {}",
            output.output
        );
        println!("{}", json!({"tool": name, "passed": true}));
    }
    println!("{}", json!({"stage":"remote_mcp_ready"}));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let cancelled = tokio::time::timeout(
        Duration::from_millis(200),
        registry.execute(expected[0], json!({"text":"fixture-delay"}), ctx.clone()),
    )
    .await;
    ensure!(cancelled.is_err(), "Delayed MCP call did not stay pending");
    let output = registry
        .execute(
            expected[0],
            json!({"text":"after cancellation"}),
            ctx.clone(),
        )
        .await?;
    ensure!(
        output.output.contains("after cancellation"),
        "Connection failed after cancellation"
    );
    println!(
        "{}",
        json!({"stage":"fusion_passed","pid":std::process::id()})
    );
    Ok(())
}

async fn verify_stdio_recovery() -> Result<()> {
    use jcode::mcp::{McpClient, McpServerConfig};
    use std::time::Duration;
    let fixture = std::env::var("JCODE_TEST_STDIO_FIXTURE")?;
    let timeout_secs = std::env::var("JCODE_TEST_MCP_TIMEOUT_SECS")
        .unwrap_or_else(|_| "10".into())
        .parse::<u64>()?;
    let config: McpServerConfig = serde_json::from_value(
        json!({"command":"node", "args":[fixture], "timeout_secs":timeout_secs}),
    )?;
    let first = McpClient::connect("stdio-regression".into(), &config).await?;
    let echo = mcp_text(
        first
            .call_tool("echo", json!({"text":"stdio works"}))
            .await?,
    )?;
    let payload: serde_json::Value = serde_json::from_str(&echo)?;
    ensure!(
        payload["sensitive_inherited"] == false,
        "MCP inherited a filtered credential variable"
    );
    let exited = tokio::time::timeout(
        Duration::from_secs(3),
        first.call_tool("echo", json!({"text":"fixture-exit"})),
    )
    .await?;
    ensure!(
        exited.is_err(),
        "Exited stdio server did not fail its pending request"
    );
    drop(first);
    let second = McpClient::connect("stdio-regression".into(), &config).await?;
    second
        .call_tool("echo", json!({"text":"after reconnect"}))
        .await?;
    println!("{}", json!({"stage":"stdio_recovery_passed"}));
    Ok(())
}

async fn verify_mcp_sessions(ctx: &ToolContext) -> Result<()> {
    use jcode::mcp::{McpConfig, McpManager, SharedMcpPool};
    let pool = Arc::new(SharedMcpPool::new(McpConfig::load_for_dir(
        ctx.working_dir.as_deref(),
    )));
    let (connected, errors) = pool.connect_all().await;
    ensure!(
        connected == 1 && errors.is_empty(),
        "Shared remote connection failed: {errors:?}"
    );
    let first = McpManager::with_shared_pool_for_dir(
        pool.clone(),
        "fusion-a".into(),
        ctx.working_dir.clone(),
    );
    let second = McpManager::with_shared_pool_for_dir(
        pool.clone(),
        "fusion-b".into(),
        ctx.working_dir.clone(),
    );
    for manager in [&first, &second] {
        let (connected, errors) = manager.connect_all().await?;
        ensure!(
            connected == 2 && errors.is_empty(),
            "Session connections failed: {errors:?}"
        );
    }
    let mut responses = Vec::new();
    for manager in [&first, &second] {
        for server in ["fixture_http", "fixture_sse"] {
            responses.push(mcp_text(
                manager
                    .call_tool(server, "echo", json!({"text":"fixture-session"}))
                    .await?,
            )?);
        }
    }
    ensure!(
        responses[0] == responses[2],
        "Shared sessions did not reuse the HTTP connection"
    );
    ensure!(
        responses[1] != responses[3],
        "Private SSE sessions shared protocol state"
    );
    ensure!(
        pool.ref_counts().await.get("fixture_http") == Some(&2),
        "Shared reference count mismatch"
    );
    let other = McpManager::with_shared_pool_for_dir(
        pool.clone(),
        "fusion-other-account".into(),
        Some(PathBuf::from(std::env::var("JCODE_TEST_OTHER_PROJECT")?)),
    );
    let (connected, errors) = other.connect_all().await?;
    ensure!(
        connected == 1 && errors.is_empty(),
        "Project override did not connect: {errors:?}"
    );
    let other_response = mcp_text(
        other
            .call_tool("fixture_http", "echo", json!({"text":"fixture-session"}))
            .await?,
    )?;
    ensure!(
        other_response != responses[0],
        "Same-named remote server with different headers reused another account's connection"
    );
    ensure!(
        pool.ref_counts().await.get("fixture_http") == Some(&2),
        "Project override acquired the global connection"
    );
    other.disconnect_all().await;
    first.disconnect_all().await;
    second.disconnect_all().await;
    pool.disconnect_all().await;
    println!("{}", json!({"stage":"mcp_session_isolation_passed"}));
    Ok(())
}

fn mcp_text(result: jcode::mcp::ToolCallResult) -> Result<String> {
    ensure!(!result.is_error, "MCP fixture returned an error");
    result
        .content
        .into_iter()
        .find_map(|block| match block {
            jcode::mcp::ContentBlock::Text { text } => Some(text),
            _ => None,
        })
        .context("MCP fixture returned no text")
}

async fn verify_remote_recovery(ctx: &ToolContext) -> Result<()> {
    use jcode::mcp::{McpClient, McpConfig};
    use std::time::Duration;
    let config = McpConfig::load_for_dir(ctx.working_dir.as_deref());
    let server = &config.servers["fixture_http"];
    let mut first = McpClient::connect("remote-reconnect".into(), server).await?;
    let disconnected = tokio::time::timeout(
        Duration::from_secs(12),
        first.call_tool("echo", json!({"text":"fixture-disconnect"})),
    )
    .await?;
    ensure!(
        disconnected.is_err(),
        "Disconnected HTTP request was reported as successful"
    );
    first.shutdown().await;
    let mut second = McpClient::connect("remote-reconnect".into(), server).await?;
    second
        .call_tool("echo", json!({"text":"after explicit reconnect"}))
        .await?;
    second.shutdown().await;
    let mut denied = server.clone();
    denied
        .headers
        .insert("X-Fixture".into(), "denied fixture".into());
    denied.timeout_secs = Some(3);
    ensure!(
        McpClient::connect("remote-denied".into(), &denied)
            .await
            .is_err(),
        "Rejected remote headers were accepted"
    );
    println!(
        "{}",
        json!({"stage":"remote_recovery_and_auth_error_passed"})
    );
    Ok(())
}

fn create_temp_workspace() -> Result<PathBuf> {
    let mut path = std::env::temp_dir();
    path.push(format!("jcode-harness-{}", new_id("run")));
    Ok(path)
}
