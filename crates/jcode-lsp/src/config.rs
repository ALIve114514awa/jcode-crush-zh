use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Explicitly configured servers are started only when a query needs them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LspConfig {
    pub enabled: bool,
    pub servers: BTreeMap<String, ServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub command: String,
    pub args: Vec<String>,
    /// Extensions without a leading dot, for example ["ts", "tsx"].
    pub file_types: Vec<String>,
    pub root_markers: Vec<String>,
    /// Leave empty to derive the LSP language ID from the extension.
    pub language_id: String,
    pub env: BTreeMap<String, String>,
    pub initialization_options: Value,
    pub settings: Value,
    pub timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub disabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            file_types: Vec::new(),
            root_markers: vec![".git".into()],
            language_id: String::new(),
            env: BTreeMap::new(),
            initialization_options: Value::Null,
            settings: Value::Null,
            timeout_secs: 15,
            idle_timeout_secs: 120,
            disabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Configuration {
        lsp: LspConfig,
    }

    #[test]
    fn omitted_config_is_disabled_and_example_options_reach_the_server() {
        assert!(!toml::from_str::<Configuration>("").unwrap().lsp.enabled);
        let config: Configuration =
            toml::from_str(include_str!("../examples/lsp-typescript.toml")).unwrap();
        assert!(config.lsp.enabled);
        let server = &config.lsp.servers["typescript"];
        assert_eq!(server.command, "node");
        assert_eq!(server.initialization_options["maxTsServerMemory"], 256);
        assert_eq!(
            server.initialization_options["tsserver"]["useSyntaxServer"],
            "never"
        );
        assert!(server.env.is_empty());
    }
}
