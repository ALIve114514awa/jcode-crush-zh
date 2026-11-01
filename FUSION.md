# JCode LSP / 远程 MCP 集成版

当前中文版本为 `0.94.0-crush.3.zhCN`。中文使用入口和汉化范围见 [README.zh-CN.md](README.zh-CN.md)。

Windows 便携打包：`scripts/package-portable.ps1 -ArtifactExePath target/selfdev/jcode.exe -DestinationPath E:\JCode-Portable`。
运行时默认从现有安装目录复用固定依赖，也可用 `-RuntimeRoot` 指定完整的集成版目录。
默认迁移现有 JCode 资料，`-SkipProfileMigration` 可创建全新资料包。已有目标目录会被拒绝，避免覆盖便携资料。
启动器以包内 `data` 为 `JCODE_HOME`，每台电脑使用独立运行目录和服务器管道，
通过 `JCODE_NODE_BINARY`（必须是现有绝对文件路径）及 `JCODE_MCP_REMOTE_BRIDGE` 指向随包运行时。
详细使用说明见 [便携版说明](scripts/portable/README.zh-CN.md)。

此分支基于 JCode `2a9e7f62481bd198685153051336051a731d86de`，沿用 JCode 的 TUI、模型接入、会话和工具执行机制，加入 LSP 查询与远程 MCP。Crush 作为功能设计参考，没有复制其源码。完整 Crush 界面和 LSP 跨文件重构不属于本版。

## Windows 安装与使用

默认程序入口是 `%LOCALAPPDATA%\jcode\bin\jcode.exe`，开始菜单提供 JCode 快捷方式。安装后重新打开终端，运行：

```powershell
jcode --version
jcode
```

配置位于 `%USERPROFILE%\.jcode\config.toml`；设置了 `JCODE_HOME` 时使用该目录。模型登录和模型选择沿用 JCode 原有流程。安装程序保留已有配置并生成备份，再添加 TypeScript / JavaScript 的语言服务器。为保留定制功能，安装时将 `features.check_updates` 设为 `false`。

程序、Node、语言服务器和远程 MCP 适配器都安装在应用目录下，运行时不需要此源码目录。工具链仅用于编译。

## LSP 查询

开启 LSP 后，模型的工具列表增加：

| 工具 | 用途 |
| --- | --- |
| `lsp_definition` | 跳转到符号定义 |
| `lsp_references` | 查找符号引用 |
| `lsp_symbols` | 查看文件中的符号 |
| `lsp_diagnostics` | 获取语法和类型诊断 |

工具参数 `file_path` 可使用项目相对路径；`line`、`column` 从 1 开始，列按 Unicode 字符计数，emoji 计为一个字符。输出保留 LSP 的零基坐标，并注明位置编码。

服务器按项目需要启动，同项目和相同配置可复用；闲置后退出。查询前同步已打开文件的磁盘变化。诊断的 `received=false` 表示尚未收到结果；`fresh=null` 表示服务器没有为诊断附带文档版本。

安装包含固定版本 TypeScript 6.0.3、typescript-language-server 6.0.2。语言服务器的 `maxTsServerMemory=256` 是 tsserver 的 V8 内存设置，不是完整进程树的内存上限。其他语言可在 `lsp.servers` 中配置已有语言服务器，参见 `crates/jcode-lsp/README.md`。

## 远程 MCP

沿用 JCode 的 MCP 配置与工具发现机制，在项目 `.mcp.json` 或 JCode 配置目录 `mcp.json` 中增加远程服务器。以下 URL 是占位示例，使用时替换为实际服务地址：

```json
{
  "mcpServers": {
    "my_remote": {
      "type": "http",
      "url": "https://your-mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer ${MY_MCP_TOKEN}" },
      "shared": true,
      "timeout_secs": 60
    }
  }
}
```

`type` 接受 `http`、`streamable-http`、`sse`，也接受字段名 `transport`。HTTP 使用 Streamable HTTP；SSE 用于旧版 SSE 端点。已有 stdio 配置保持原有形式。

远程连接通过随程序安装的 **mcp-remote 0.15.1** 适配到现有 stdio MCP 客户端。因此每个远程连接增加一个 Node 适配进程；共享连接由 JCode 原有池管理，`shared=false` 的连接由各会话分别拥有。同名远程服务器的 URL、headers 或其他配置不同时，项目覆盖会使用独立连接。

为保持已有 stdio 配置的兼容性，后加载的同名远程条目不会替换先前的 stdio 条目；切换传输时应修改原条目，或使用不同的服务器名称。

需要 OAuth 的服务会打开默认浏览器。认证缓存放在 JCode 配置目录的 `mcp-auth` 中。未设置超时时，远程初始化允许最多 120 秒，普通工具请求仍使用 JCode 的 30 秒默认值；显式 `timeout_secs` 同时控制初始化和请求。

需要代理时，可在该服务器的 `env` 中设置 `HTTPS_PROXY` / `HTTP_PROXY`；本地服务可通过 `NO_PROXY` 排除代理。

请求取消和超时会清理等待项并发送 MCP 取消通知。取消后是否立即停止远程工作取决于服务器。调用失败时不自动重放可能有副作用的工具；可通过 JCode 原有 MCP 管理工具重新连接。

## 从源码构建与安装

Windows 编译需要 Rust 及匹配的原生 C/C++ 工具链。GNU LLVM 构建需要 LLVM-MinGW，依赖使用的构建工具还包括 CMake、Ninja。此机器的便携工具链配置保存在源码目录旁的 `.tools/enter-cargo.ps1`。该工具链的 Rust 链接目录已补入 LLVM-MinGW 中缺失的 Windows 导入库，原有文件保留；AWS-LC 使用其提供的 `EFFECTIVE_TARGET=x86_64-pc-windows-gnu` 和预编译 NASM 对象配置，复用相同 MinGW ABI 的绑定。

GNU LLVM 目标下的 QuickJS 使用 `bindgen` 生成当前头文件的绑定，需要设置 `LIBCLANG_PATH`，并让绑定生成器使用同一 LLVM-MinGW 的头文件。其他 Windows 目标继续使用依赖附带的预生成绑定。本工作区使用便携的 libclang 18.1.1。

```powershell
# 如果使用本工作区的便携工具链：
. ..\.tools\enter-cargo.ps1

cargo --config 'build.rustc-wrapper=""' build --bin jcode --no-default-features --profile selfdev
cargo --config 'build.rustc-wrapper=""' build --bin jcode-harness --no-default-features --profile selfdev

Push-Location scripts\fusion-runtime
npm ci --ignore-scripts --no-audit --no-fund
Pop-Location

.\scripts\install-fusion.ps1 -ArtifactExePath .\target\selfdev\jcode.exe `
  -RuntimeDllDir ..\.tools\rustup\toolchains\stable-x86_64-pc-windows-gnullvm\bin
```

此构建方式不启用默认的 PDF 与 Bedrock 可选特性，也不包含本地 embedding。需要这些特性时应另行构建并验证其依赖。

## 验证入口

```powershell
python scripts\stage_lsp_validation.py
cargo --config 'build.rustc-wrapper=""' test --manifest-path .tmp\lsp-validation\Cargo.toml --target-dir target\lsp-validation -p jcode-lsp -p jcode-mcp-bridge

python scripts\verify_remote_mcp.py scripts\fusion-runtime\node_modules\mcp-remote\dist\proxy.js --output .tmp\remote-mcp.json
python scripts\verify_fusion_host.py target\selfdev\jcode-harness.exe scripts\fusion-runtime --output .tmp\fusion-host.json
```

协议测试使用本地可控服务。宿主测试经实际 Registry 执行文件写入、编辑、LSP 查询和 MCP 调用，使用独立配置及会话目录，不调用模型服务。报告记录测试二进制哈希、整个子进程树的内存采样和退出后的子进程状态；不同测试程序的资源数字不能互相替代。

依赖的许可证保留在安装目录对应的 npm 包中；Node 与 JCode 的许可证随安装保留。
