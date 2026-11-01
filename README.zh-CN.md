# JCode 中文集成版

这是基于 JCode 的简体中文终端编码代理，加入 LSP 查询和远程 MCP，适合使用远程模型的 Windows 环境。保留 JCode 的模型、会话和工具执行机制，Crush 仅作为能力设计参考，没有复制 Crush 源码。

## 已实现功能

- 中文首次使用引导、常用命令帮助与补全、登录和账户选择、模型选择、会话恢复、用量与权限界面，以及常见运行状态提示。
- TypeScript / JavaScript 的定义、引用、符号和诊断查询，按项目需要启动语言服务器。
- Streamable HTTP 和旧版 SSE 远程 MCP，通过随附的 mcp-remote 适配器接入原有工具系统，支持请求头认证与 OAuth。
- 保留 JCode 现有的读写文件、编辑、命令执行、搜索和会话持久化能力。

命令名、参数、配置键、模型 ID 和协议字段保持原有形式，例如 `/model`、`/login openai`。模型回复、远程工具返回内容、第三方服务错误和部分开发诊断按原始内容显示；它们不经过全局文本替换。此版本尚未接入完整 Crush 界面或 LSP 跨文件重构。

## Windows 使用

需要放到 U 盘时，可用 `scripts/package-portable.ps1` 打包 Windows x64 便携版。
包内的“启动 JCode.cmd”会将配置、登录、会话和日志保存到旁边的 `data`，
并随盘符变化重新定位内置工具。默认项目目录为 `Projects`，也支持将项目文件夹
拖到启动入口上。详细说明见 [便携版使用说明](scripts/portable/README.zh-CN.md)。

默认入口：`%LOCALAPPDATA%\jcode\bin\jcode.exe`。安装后在开始菜单打开 **JCode**，或重新打开终端：

```powershell
jcode --version
jcode
```

首次运行按中文引导选择服务商。也可以在界面输入 `/login` 登录，再通过 `/model` 选择模型。GitHub 登录仅用于管理源码仓库，与 JCode 的模型登录分别管理。

| 命令 | 用途 |
| --- | --- |
| `/help` | 查看命令和快捷键 |
| `/help model` | 查看指定命令的详细说明 |
| `/login` | 登录模型服务商 |
| `/model` | 选择或切换模型 |
| `/account` | 管理多个账户 |
| `/resume` | 搜索并恢复历史会话 |
| `/usage` | 查询服务商用量 |
| `/config edit` | 编辑配置 |
| `/compact` | 总结旧消息以释放上下文 |
| `/quit` | 退出界面 |

配置默认在 `%USERPROFILE%\.jcode\config.toml`。程序随附 Node、TypeScript、语言服务器和 MCP 适配器，不需要源码目录或编译工具链即可运行。语言服务器按需启动；远程 MCP 每个连接会增加一个 Node 进程。

为保留定制功能，集成安装关闭了上游自动更新检查。使用 `jcode update` 可能安装上游版本，应使用本仓库的构建与安装脚本更新中文集成版。

## 构建、安装和验证

完整命令、远程 MCP 配置、固定依赖版本和资源测量口径见 [集成使用说明](FUSION.md)。Windows 使用本地构建产物安装：

```powershell
.\scripts\install-fusion.ps1 -ArtifactExePath .\target\selfdev\jcode.exe `
  -RuntimeDllDir ..\.tools\rustup\toolchains\stable-x86_64-pc-windows-gnullvm\bin
```

安装器合并已有配置并保留备份，添加开始菜单入口和用户 PATH。GNU LLVM 构建的 `libunwind.dll` 会放在程序旁。当前精简构建不启用 PDF、Bedrock 和本地 embedding 特性。

上游基线为 `2a9e7f62481bd198685153051336051a731d86de`。JCode 保留 MIT 许可证及原作者归属；依赖许可证保留在对应包目录。上游英文文档仍可在 [README](README.md) 查看。
