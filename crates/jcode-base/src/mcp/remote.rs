//! Adapt HTTP/SSE servers to the existing MCP handles, policies and shared pool.
use super::protocol::McpServerConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub(super) fn bridge_config(config: &McpServerConfig) -> Result<McpServerConfig> {
    let entry = if let Some(path) = std::env::var_os("JCODE_MCP_REMOTE_BRIDGE") {
        PathBuf::from(path)
    } else {
        dirs::data_local_dir()
            .context("Cannot locate the per-user application directory for the remote MCP adapter")?
            .join("jcode/tools/runtime/node_modules/mcp-remote/dist/proxy.js")
    };
    let launch = jcode_mcp_bridge::launch(
        &entry,
        config.url.as_deref().context("Remote MCP URL is missing")?,
        config.transport.as_deref().unwrap_or("http"),
        &config.headers,
        config.timeout_secs.unwrap_or(120),
    )?;
    let mut stdio = config.clone();
    let bundled_node = dirs::data_local_dir().map(|dir| dir.join("jcode/tools/node/node.exe"));
    stdio.command = bundled_node
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "node".into());
    stdio.args = launch.args;
    stdio.env.extend(launch.env);
    let auth_dir = crate::storage::jcode_dir()?.join("mcp-auth");
    stdio
        .env
        .entry("MCP_REMOTE_CONFIG_DIR".into())
        .or_insert_with(|| auth_dir.to_string_lossy().into_owned());
    stdio.transport = Some("stdio".into());
    Ok(stdio)
}
