# JCode LSP queries

First P1 implementation from the JCode/Crush integration handoff. JCode owns
the agent loop, sessions and tool execution. This Rust module implements the
LSP client independently; it does not copy Crush source.

## Configuration

LSP is disabled by default. Merge `examples/lsp-typescript.toml` into
`JCODE_HOME/config.toml` (normally `~/.jcode/config.toml`) and create a new
session. Edit the absolute server paths if using another checkout. The
example uses Node to launch the server directly, including paths with spaces
on Windows; it does not depend on shell parsing of a `.cmd` shim.

This checkout has TypeScript 6.0.3 and typescript-language-server 6.0.2 under
`../.tools/lsp-node/node_modules`, with Node 24.18.0. For another installation:

```powershell
npm install --prefix <local-tools-directory> --ignore-scripts typescript@6.0.3 typescript-language-server@6.0.2
```

Only servers explicitly listed in `[lsp.servers]` run. A query selects the first
enabled server, sorted by name, that matches the file extension. Project roots
use the nearest configured root marker, bounded by the session working
directory. Identical server configuration and canonical project root share an
instance across sessions; differing options or roots use separate instances.
Servers start on the first query and shut down after `idle_timeout_secs`
(default 120 seconds; the cleanup loop checks once per second).

The TypeScript example disables automatic type acquisition and the separate
syntax server, and sets a 256 MiB V8 old-space limit. That value is not a limit
on the memory of the entire process tree. Actual measurements belong in the
acceptance record.

## Tools

Tools register through the existing JCode `Registry`, retaining its policies,
input transformations, hooks, cancellation and output handling.

| Tool | Arguments beyond common `intent` | Result |
| --- | --- | --- |
| `lsp_definition` | `file_path`, `line`, `column` | Location or location-link result |
| `lsp_references` | `file_path`, `line`, `column`, optional `include_declaration` | Reference locations |
| `lsp_symbols` | `file_path` | Document symbols |
| `lsp_diagnostics` | `file_path`, optional `wait_ms` from 0 to 5000 | Push or pull diagnostics |

Input lines and columns start at **1**; columns count Unicode scalar values
(an emoji counts as one). Response `data` preserves the server's raw LSP
positions: **zero-based**, in the stated `position_encoding` (`utf-16`,
`utf-8` or `utf-32`). The envelope also reports the server, project root,
document version, `line_base` and `character_base`.

For push diagnostics, distinguish `received` from `fresh`:

- `received=false` means no publication arrived before the wait window; empty
  `items` in that case does not establish that the file has no errors.
- `fresh=true` means the publication matches the current document version.
- `fresh=null` means the server omitted the version, so freshness is unknown.

Previous-version publications are discarded. Any synchronized file change or
close invalidates cached diagnostics across the server, because dependencies
can change without incrementing the target's document version. A same-version
publication cannot independently establish dependency freshness.

Files must be readable UTF-8, inside the session working directory, and at
most 4 MiB. Before querying, the client refreshes the target and all previously
queried files, sending `didOpen`, full or incremental `didChange`, requested
`didSave`, and `didClose` when previously opened files disappear. At most 64
documents stay open per server. Changes to files never queried depend on the
server's own disk watching; this version does not add a workspace watcher.

Queries are serialized per server. Cancellation and request timeouts remove
their pending waiter and send `$/cancelRequest`. Disconnects fail pending
calls; the next query starts a new server. Missing executables, unsupported
capabilities and initialization failures produce errors through the normal
tool path. On Windows, a Job object terminates descendants such as tsserver
when the client is released. Non-Windows currently uses direct child cleanup.

No LSP query applies workspace edits. A server's `workspace/applyEdit` request
receives `applied=false`. Semantic writes, remote MCP, and Crush UI adaptation
are separate later phases.

## Validation and standalone queries

Protocol tests require Node on PATH and use `tests/fixtures/server.mjs`, not
an external model or shared JCode daemon:

```powershell
cargo test -p jcode-lsp -- --test-threads=1
cargo clippy -p jcode-lsp --all-targets -- -D warnings
cargo fmt -p jcode-lsp --check
```

The full upstream workspace resolves unrelated Git dependencies even when a
single package is selected. If those downloads are unavailable, stage the
five-crate local dependency closure with source hashes and test that copy:

```powershell
python scripts/stage_lsp_validation.py
cargo --config 'build.rustc-wrapper=""' test --offline --manifest-path .tmp/lsp-validation/Cargo.toml --target-dir target/lsp-validation -p jcode-lsp -- --test-threads=1
cargo clippy --config 'build.rustc-wrapper=""' --offline --manifest-path .tmp/lsp-validation/Cargo.toml --target-dir target/lsp-validation -p jcode-lsp --all-targets -- -D warnings
cargo --config 'build.rustc-wrapper=""' build --offline --manifest-path .tmp/lsp-validation/Cargo.toml --target-dir target/lsp-validation -p jcode-lsp --examples
```

This fallback validates the LSP module and its real local dependencies; it does
not validate the three integration points in the full application. Preserve
that distinction when reporting results. The POSIX upstream `rustc-wrapper`
is disabled above for this Windows environment. The portable GNU LLVM Rust
toolchain also needs `CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER` set to its
bundled `rust-lld.exe`; `../.tools/enter-cargo.ps1` supplies this checkout's local
toolchain environment and limits compilation to one job.

Run the built standalone query example:

```powershell
target/lsp-validation/debug/examples/query.exe crates/jcode-lsp/examples/lsp-typescript.toml <project-directory> <file.ts> definition 2 24
```

Or validate all four query types, CRLF, Chinese/emoji paths and columns,
disk edits and cross-file diagnostics using the real TypeScript server:

```powershell
target/lsp-validation/debug/examples/typescript_smoke.exe ../.tools/lsp-node/node_modules/typescript-language-server/lib/cli.mjs ../.tools/lsp-node/node_modules/typescript/lib/tsserver.js
```

The Windows measurement script runs that smoke test, sampling the harness and
all descendants every 200 ms and checking observed children after shutdown:

```powershell
python scripts/measure_lsp_windows.py target/lsp-validation/debug/examples/typescript_smoke.exe ../.tools/lsp-node/node_modules/typescript-language-server/lib/cli.mjs ../.tools/lsp-node/node_modules/typescript/lib/tsserver.js --output .tmp/lsp-validation-results/typescript-resources.json
```

Do not treat harness measurements as the memory footprint of the full JCode
binary, daemon or TUI. For full-host runtime tests, use the new executable
with separate `JCODE_HOME`, session storage and IPC endpoints as required by
`AGENTS.md`.
