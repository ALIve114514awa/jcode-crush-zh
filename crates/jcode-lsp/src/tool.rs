use crate::{LspConfig, Manager, Query};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use jcode_tool_core::{Tool, ToolContext};
use jcode_tool_types::ToolOutput;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, LazyLock, Mutex, Weak};

static MANAGER: LazyLock<Mutex<Weak<Manager>>> = LazyLock::new(|| Mutex::new(Weak::new()));

#[derive(Clone, Copy)]
enum Kind {
    Definition,
    References,
    Symbols,
    Diagnostics,
}

struct LspTool {
    kind: Kind,
    config: Arc<LspConfig>,
    manager: Arc<Manager>,
}

/// Server configuration is captured when a session creates its tool set.
/// The shared manager reuses only identical configurations in the same project.
pub fn configured_tools(config: LspConfig) -> Vec<Arc<dyn Tool>> {
    if !config.enabled {
        return Vec::new();
    }
    let manager = {
        let mut shared = MANAGER.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(manager) = shared.upgrade() {
            manager
        } else {
            let manager = Arc::new(Manager::new());
            *shared = Arc::downgrade(&manager);
            manager
        }
    };
    let config = Arc::new(config);
    [
        Kind::Definition,
        Kind::References,
        Kind::Symbols,
        Kind::Diagnostics,
    ]
    .into_iter()
    .map(|kind| {
        Arc::new(LspTool {
            kind,
            config: config.clone(),
            manager: manager.clone(),
        }) as Arc<dyn Tool>
    })
    .collect()
}

#[derive(Deserialize)]
struct Input {
    file_path: String,
    line: Option<usize>,
    column: Option<usize>,
    #[serde(default = "yes")]
    include_declaration: bool,
    #[serde(default = "default_wait")]
    wait_ms: u64,
}
fn yes() -> bool {
    true
}
fn default_wait() -> u64 {
    1000
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        match self.kind {
            Kind::Definition => "lsp_definition",
            Kind::References => "lsp_references",
            Kind::Symbols => "lsp_symbols",
            Kind::Diagnostics => "lsp_diagnostics",
        }
    }

    fn description(&self) -> &str {
        match self.kind {
            Kind::Definition => {
                "Find a symbol's definition using a configured language server. Input line and column start at 1; columns count Unicode characters, including emoji as one character. Response data uses zero-based LSP positions in the reported position_encoding. Read-only; starts the server on demand."
            }
            Kind::References => {
                "Find references at a file position using a configured language server. Input line and column start at 1 and count Unicode characters. Response data uses zero-based LSP positions in the reported position_encoding. Read-only; synchronizes previously queried files before querying."
            }
            Kind::Symbols => {
                "List a file's document symbols using a configured language server. Response data uses zero-based LSP positions in the reported position_encoding. Read-only; starts the server on demand."
            }
            Kind::Diagnostics => {
                "Get a file's language-server diagnostics after synchronizing disk changes. Checks received and fresh separately: received=false means the server has not published diagnostics, not that the file has no errors; fresh=null means a versionless publication. Response positions are zero-based in the reported position_encoding. Read-only."
            }
        }
    }

    fn parameters_schema(&self) -> Value {
        let mut schema = json!({"type":"object","properties":{
            "file_path":{"type":"string","description":"UTF-8 source file, absolute or relative to the working directory"}
        },"required":["file_path"]});
        if matches!(self.kind, Kind::Definition | Kind::References) {
            schema["properties"]["line"] =
                json!({"type":"integer","minimum":1,"description":"1-based line number"});
            schema["properties"]["column"] = json!({"type":"integer","minimum":1,"description":"1-based column, counting Unicode characters"});
            schema["required"] = json!(["file_path", "line", "column"]);
        }
        if matches!(self.kind, Kind::References) {
            schema["properties"]["include_declaration"] = json!({"type":"boolean","default":true});
        }
        if matches!(self.kind, Kind::Diagnostics) {
            schema["properties"]["wait_ms"] = json!({"type":"integer","minimum":0,"maximum":5000,"default":1000,"description":"Maximum wait for push diagnostics"});
        }
        schema
    }

    async fn execute(&self, input: Value, context: ToolContext) -> Result<ToolOutput> {
        let input: Input = serde_json::from_value(input).context("Invalid LSP query input")?;
        if input.file_path.trim().is_empty() {
            bail!("file_path must not be empty");
        }
        let query = match self.kind {
            Kind::Definition => Query::Definition {
                line: input.line.context("line is required")?,
                column: input.column.context("column is required")?,
            },
            Kind::References => Query::References {
                line: input.line.context("line is required")?,
                column: input.column.context("column is required")?,
                include_declaration: input.include_declaration,
            },
            Kind::Symbols => Query::Symbols,
            Kind::Diagnostics => Query::Diagnostics {
                wait_ms: input.wait_ms,
            },
        };
        let workspace = match context.working_dir {
            Some(directory) => directory,
            None => std::env::current_dir()?,
        };
        let call = self.manager.query(
            &self.config,
            &workspace,
            std::path::Path::new(&input.file_path),
            query,
        );
        let result = if let Some(signal) = context.graceful_shutdown_signal {
            tokio::select! {
                biased;
                _ = signal.notified() => bail!("LSP query cancelled"),
                result = call => result?,
            }
        } else {
            call.await?
        };
        Ok(ToolOutput::new(serde_json::to_string_pretty(&result)?).with_title(self.name()))
    }
}
