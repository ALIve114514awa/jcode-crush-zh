//! Standalone query harness: no model credentials or shared JCode daemon.
use anyhow::{Context, Result, bail};
use jcode_lsp::{LspConfig, Manager, Query};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Configuration {
    lsp: LspConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        bail!(
            "Usage: query <config.toml> <working-directory> <file> <symbols|diagnostics|definition|references> [line column]"
        );
    }
    let config: Configuration = toml::from_str(&std::fs::read_to_string(&args[0])?)?;
    let coordinate = |index: usize| -> Result<usize> {
        args.get(index)
            .context("line and column are required")?
            .parse()
            .context("Coordinates must be positive integers")
    };
    let query = match args[3].as_str() {
        "symbols" => Query::Symbols,
        "diagnostics" => Query::Diagnostics { wait_ms: 1000 },
        "definition" => Query::Definition {
            line: coordinate(4)?,
            column: coordinate(5)?,
        },
        "references" => Query::References {
            line: coordinate(4)?,
            column: coordinate(5)?,
            include_declaration: true,
        },
        _ => bail!("Unknown query type"),
    };
    let manager = Manager::new();
    let result = manager
        .query(&config.lsp, Path::new(&args[1]), Path::new(&args[2]), query)
        .await;
    manager.shutdown().await;
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}
