//! Exercise a real TypeScript server without an API key or a JCode daemon.
//! Optional JCODE_LSP_MEASURE=1 waits on stdin at baseline and before shutdown.
use anyhow::{Context, Result, ensure};
use jcode_lsp::{LspConfig, Manager, Query, ServerConfig};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Write, path::Path};

fn emit(stage: &str, data: Value) -> Result<()> {
    println!(
        "{}",
        json!({"stage":stage,"pid":std::process::id(),"data":data})
    );
    std::io::stdout().flush()?;
    Ok(())
}

fn measurement_gate() -> Result<()> {
    if std::env::var("JCODE_LSP_MEASURE").as_deref() == Ok("1") {
        let mut line = String::new();
        ensure!(
            std::io::stdin().read_line(&mut line)? > 0,
            "Measurement controller disconnected"
        );
    }
    Ok(())
}

fn executable_argument(path: &str) -> Result<String> {
    let path = Path::new(path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    ensure!(
        absolute.is_file(),
        "Server script does not exist: {}",
        absolute.display()
    );
    // Windows canonicalize adds a verbatim prefix that Node's entry-point
    // resolver rejects. Resolve relative paths without introducing that prefix.
    Ok(absolute.to_string_lossy().into_owned())
}

fn definition_uri(data: &Value) -> Option<&str> {
    let location = data
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(data);
    location["uri"]
        .as_str()
        .or_else(|| location["targetUri"].as_str())
}

async fn verify(manager: &Manager, config: &LspConfig, root: &Path) -> Result<()> {
    let definitions = root.join("defs.ts");
    let file = root.join("调用 空格😀.ts");
    let source = "import { greet } from './defs.js';\r\nconst 文本 = '中文😀'; greet(文本);\r\nconst broken: number = 'wrong';\r\n";
    std::fs::write(
        &definitions,
        "export function greet(name: string): string {\r\n  return 'Hello ' + name;\r\n}\r\n",
    )?;
    std::fs::write(&file, source)?;

    let symbols = manager
        .query(config, root, &definitions, Query::Symbols)
        .await?;
    ensure!(
        symbols["data"]
            .as_array()
            .is_some_and(|items| items.iter().any(|s| s["name"] == "greet")),
        "Missing greet document symbol"
    );
    emit("definition_symbols", symbols)?;

    let second_line = source.lines().nth(1).context("Missing fixture line")?;
    let prefix = &second_line[..second_line
        .find("greet")
        .context("Missing fixture symbol")?];
    let column = prefix.chars().count() + 1;
    let definition = manager
        .query(config, root, &file, Query::Definition { line: 2, column })
        .await?;
    let uri = definition_uri(&definition["data"]).context("Missing definition URI")?;
    let target = url::Url::parse(uri)?
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Invalid definition file URI"))?;
    ensure!(
        std::fs::canonicalize(target)? == std::fs::canonicalize(&definitions)?,
        "Definition did not target defs.ts"
    );
    ensure!(
        definition["position_encoding"] == "utf-16",
        "Unexpected TypeScript position encoding"
    );
    emit("definition", definition)?;

    let references = manager
        .query(
            config,
            root,
            &file,
            Query::References {
                line: 2,
                column,
                include_declaration: true,
            },
        )
        .await?;
    ensure!(
        references["data"]
            .as_array()
            .is_some_and(|items| items.len() >= 3),
        "Expected declaration, import and call references"
    );
    emit("references", references)?;
    let symbols = manager.query(config, root, &file, Query::Symbols).await?;
    ensure!(
        symbols["data"]
            .as_array()
            .is_some_and(|items| items.iter().any(|s| s["name"] == "broken")),
        "Missing broken document symbol"
    );
    emit("symbols", symbols)?;

    let diagnostics = manager
        .query(config, root, &file, Query::Diagnostics { wait_ms: 5000 })
        .await?;
    ensure!(
        diagnostics["data"]["received"] == true,
        "TypeScript did not publish diagnostics"
    );
    ensure!(
        diagnostics["data"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|d| d["code"] == 2322)),
        "Expected TypeScript type error 2322"
    );
    emit("diagnostics_before_edit", diagnostics)?;

    // Model an edit by an existing JCode file tool, then query the same server.
    std::fs::write(&file, source.replace("'wrong'", "42"))?;
    let diagnostics = manager
        .query(config, root, &file, Query::Diagnostics { wait_ms: 5000 })
        .await?;
    ensure!(
        diagnostics["document_version"] == 2,
        "Disk change did not increment the document version"
    );
    ensure!(
        diagnostics["data"]["received"] == true,
        "No diagnostic publication after edit"
    );
    ensure!(
        diagnostics["data"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().all(|d| d["severity"] != 1)),
        "Type errors remained after fixing the file"
    );
    emit("diagnostics_after_edit", diagnostics)?;

    // A previously queried file must also be synchronized before a new query.
    std::fs::write(
        &definitions,
        "export function greet(name: number): string {\r\n  return 'Hello ' + name;\r\n}\r\n",
    )?;
    let diagnostics = manager
        .query(config, root, &file, Query::Diagnostics { wait_ms: 5000 })
        .await?;
    ensure!(
        diagnostics["data"]["received"] == true,
        "No diagnostics after cross-file edit"
    );
    ensure!(
        diagnostics["data"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|d| d["code"] == 2345)),
        "Expected cross-file type error 2345"
    );
    emit("diagnostics_after_dependency_edit", diagnostics)?;
    ensure!(
        manager.running_servers().await == 1,
        "Queries did not reuse a single server"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2,
        "Usage: typescript_smoke <language-server-cli.mjs> <tsserver.js>"
    );
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("中文 项目😀");
    std::fs::create_dir(&root)?;
    std::fs::write(root.join("package.json"), "{\"type\":\"module\"}")?;
    std::fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec(&json!({
            "compilerOptions":{"strict":true,"noEmit":true,"target":"ES2022","module":"NodeNext","moduleResolution":"NodeNext","types":[]},
            "include":["*.ts"]
        }))?,
    )?;
    let config = LspConfig {
        enabled: true,
        servers: BTreeMap::from([(
            "typescript".into(),
            ServerConfig {
                command: "node".into(),
                args: vec![executable_argument(&args[0])?, "--stdio".into()],
                file_types: vec!["ts".into()],
                root_markers: vec!["tsconfig.json".into()],
                initialization_options: json!({
                    "disableAutomaticTypingAcquisition":true,
                    "maxTsServerMemory":256,
                "tsserver":{"path":executable_argument(&args[1])?,"useSyntaxServer":"never"}
                }),
                timeout_secs: 30,
                idle_timeout_secs: 30,
                ..Default::default()
            },
        )]),
    };
    let manager = Manager::new();
    emit(
        "baseline",
        json!({"servers":manager.running_servers().await,"root":root}),
    )?;
    measurement_gate()?;
    let result = verify(&manager, &config, &root).await;
    if result.is_ok() {
        emit("active", json!({"servers":manager.running_servers().await}))?;
        measurement_gate()?;
    }
    manager.shutdown().await;
    result?;
    emit(
        "shutdown",
        json!({"servers":manager.running_servers().await}),
    )?;
    Ok(())
}
