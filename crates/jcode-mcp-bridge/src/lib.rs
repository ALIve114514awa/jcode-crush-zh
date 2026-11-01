//! Launch descriptions for the pinned remote MCP adapter bundled by the installer.
//! Configured header values travel in the environment, not in process arguments.
use anyhow::{Context, Result, bail};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use url::Url;

pub const ADAPTER_VERSION: &str = "0.15.1";

#[derive(Debug)]
pub struct Launch {
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// An explicit portable runtime must never silently fall back to the host Node.
pub fn node_command(explicit: Option<PathBuf>, bundled: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if !path.is_absolute() || !path.is_file() {
            bail!("JCODE_NODE_BINARY must point to an existing absolute Node executable path");
        }
        return Ok(path);
    }
    Ok(bundled
        .filter(|path| path.is_file())
        .unwrap_or_else(|| "node".into()))
}

pub fn supported_remote(transport: Option<&str>, endpoint: Option<&str>) -> bool {
    let Some(endpoint) = endpoint else {
        return false;
    };
    let Some(kind) = transport else { return false };
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "http" | "streamable-http" | "sse"
    ) && valid_endpoint(endpoint).is_ok()
}

fn valid_endpoint(endpoint: &str) -> Result<Url> {
    let url = Url::parse(endpoint).context("Remote MCP URL is invalid")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("Remote MCP URL must use HTTP or HTTPS");
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        bail!(
            "Remote MCP URL must not contain user credentials or a fragment; use headers for authentication"
        );
    }
    Ok(url)
}

pub fn launch(
    entry: &Path,
    endpoint: &str,
    transport: &str,
    headers: &HashMap<String, String>,
    timeout_secs: u64,
) -> Result<Launch> {
    let endpoint = valid_endpoint(endpoint)?;
    if !entry.is_file() {
        bail!(
            "Remote MCP adapter is missing. Run scripts/install-fusion.ps1, or set JCODE_MCP_REMOTE_BRIDGE to mcp-remote's dist/proxy.js"
        );
    }
    let transport = match transport.to_ascii_lowercase().as_str() {
        "http" | "streamable-http" => "http-only",
        "sse" => "sse-only",
        _ => bail!("Unsupported remote MCP transport"),
    };
    let mut args = vec![
        entry.to_string_lossy().into_owned(),
        endpoint.to_string(),
        "--transport".into(),
        transport.into(),
        "--enable-proxy".into(),
        "--auth-timeout".into(),
        timeout_secs.max(1).to_string(),
    ];
    if endpoint.scheme() == "http" {
        args.push("--allow-http".into());
    }
    let mut env = HashMap::new();
    let mut headers: Vec<_> = headers.iter().collect();
    headers.sort_by(|a, b| a.0.cmp(b.0));
    for (index, (name, value)) in headers.into_iter().enumerate() {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            bail!("Invalid remote MCP header name");
        }
        if value.contains(['\r', '\n', '\0']) {
            bail!("Remote MCP header values must not contain line breaks or NUL");
        }
        let variable = format!("JCODE_REMOTE_HEADER_{index}");
        args.extend(["--header".into(), format!("{name}:${{{variable}}}")]);
        env.insert(variable, value.clone());
    }
    Ok(Launch { args, env })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_node_overrides_host_and_rejects_missing_or_relative_paths() {
        let directory = tempfile::tempdir().unwrap();
        let portable = directory.path().join("中文 runtime").join("node.exe");
        std::fs::create_dir_all(portable.parent().unwrap()).unwrap();
        std::fs::write(&portable, b"fixture").unwrap();
        let host = directory.path().join("host-node.exe");
        std::fs::write(&host, b"fixture").unwrap();
        assert_eq!(
            node_command(Some(portable.clone()), Some(host.clone())).unwrap(),
            portable
        );
        assert!(
            node_command(
                Some(directory.path().join("missing.exe")),
                Some(host.clone())
            )
            .is_err()
        );
        assert!(node_command(Some("node.exe".into()), Some(host.clone())).is_err());
        assert_eq!(node_command(None, Some(host.clone())).unwrap(), host);
        assert_eq!(node_command(None, None).unwrap(), PathBuf::from("node"));
    }

    #[test]
    fn remote_configs_validate_url_and_transport() {
        assert!(supported_remote(
            Some("HTTP"),
            Some("https://example.com/mcp")
        ));
        assert!(supported_remote(
            Some("sse"),
            Some("http://127.0.0.1:3000/sse")
        ));
        assert!(!supported_remote(Some("http"), Some("file:///secret")));
        assert!(!supported_remote(
            Some("http"),
            Some("https://user:secret@example.com/mcp")
        ));
        assert!(!supported_remote(
            Some("http"),
            Some("https://example.com/mcp#fragment")
        ));
        assert!(!supported_remote(
            Some("unknown"),
            Some("https://example.com/mcp")
        ));
        assert!(!supported_remote(None, Some("https://example.com/mcp")));
    }

    #[test]
    fn auth_headers_stay_out_of_arguments_and_use_direct_node_execution() {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("proxy with spaces.js");
        std::fs::write(&entry, "").unwrap();
        let result = launch(
            &entry,
            "https://example.com/mcp",
            "http",
            &HashMap::from([
                ("Authorization".into(), "Bearer a-secret with spaces".into()),
                ("X-Tenant".into(), "tenant-2".into()),
            ]),
            120,
        )
        .unwrap();
        assert_eq!(result.args[0], entry.to_string_lossy());
        assert!(result.args.iter().any(|a| a == "http-only"));
        assert!(!result.args.join(" ").contains("a-secret"));
        assert!(!result.args.join(" ").contains("tenant-2"));
        assert_eq!(
            result.env["JCODE_REMOTE_HEADER_0"],
            "Bearer a-secret with spaces"
        );
        assert!(
            result
                .args
                .iter()
                .any(|a| a == "Authorization:${JCODE_REMOTE_HEADER_0}")
        );
    }

    #[test]
    fn sse_missing_adapter_and_header_injection_are_handled() {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("proxy.js");
        assert!(
            launch(
                &entry,
                "https://example.com/mcp",
                "http",
                &HashMap::new(),
                10
            )
            .unwrap_err()
            .to_string()
            .contains("adapter is missing")
        );
        std::fs::write(&entry, "").unwrap();
        let result = launch(
            &entry,
            "http://127.0.0.1:3000/sse",
            "sse",
            &HashMap::new(),
            10,
        )
        .unwrap();
        assert!(result.args.iter().any(|a| a == "sse-only"));
        assert!(result.args.iter().any(|a| a == "--allow-http"));
        assert!(
            launch(
                &entry,
                "https://example.com/mcp",
                "http",
                &HashMap::from([("X-Header".into(), "value\r\nInjected: secret".into())]),
                10
            )
            .is_err()
        );
    }
}
