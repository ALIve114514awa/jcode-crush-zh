use super::*;

impl App {
    pub(super) fn command_help(&self, topic: &str) -> Option<String> {
        let topic = topic.trim().trim_start_matches('/').to_lowercase();
        let help = match topic.as_str() {
            "help" | "commands" => {
                "/help\n查看常用命令和快捷键。\n\n/help <command>\n查看指定命令的详细说明。命令名与参数保持英文。"
            }
            "compact" => {
                "/compact\n立即在后台总结旧消息，并在准备好后应用压缩结果。\n\n/compact mode\n查看当前会话的压缩模式。\n\n/compact mode <reactive|proactive|semantic>\n设置此会话的压缩模式。"
            }
            "cache" => {
                "/cache 或 /cache stats\n查看此会话的缓存读写、命中率、保留策略和最近未命中原因，不修改设置。\n\n/cache extend 或 /cache 1h\n为后续请求和会话保存 Anthropic 一小时缓存设置，写入费用为基础输入 Token 的两倍。\n\n/cache 5m\n保存五分钟缓存设置；已有缓存不会因此延长。OpenAI 的缓存保留由服务商和模型管理。"
            }
            "fix" => {
                "/fix\n尝试恢复无法继续的会话：补全缺失的工具结果、重置服务商会话状态，并在可能时压缩上下文。"
            }
            "rewind" => {
                "/rewind\n显示带编号的对话历史。\n\n/rewind N\n回退到第 N 条消息，移除其后的消息并重置服务商会话。\n\n/rewind undo\n撤销最近一次回退并恢复被移除的消息。"
            }
            "clear" => "/clear\n清空当前对话、队列和界面，开始新会话。",
            "cls" | "clear-view" => {
                "/cls\n仅清空当前显示，模型仍保留完整上下文。Ctrl+L 清屏后，历史仍保留在终端滚动区。"
            }
            "model" => {
                "/model\n打开模型选择器。\n\n/model <name>\n切换模型。\n\n/model <name>@<provider>\n为 OpenRouter 固定路由；@auto 清除固定路由。"
            }
            "provider-test-coverage"
            | "provider test coverage"
            | "model-status"
            | "model status" => {
                "/provider-test-coverage\nShow jcode live verification evidence for the current provider/model.\n\n/provider-test-coverage <provider> <model>\nLook up a specific provider/model pair in the live-test coverage ledger.\n\nThe report shows last-tested time, jcode build, passed/missing checkpoints, readiness gaps, and a caveat that missing evidence is not a provider failure."
            }
            "refresh-model-list" => {
                "/refresh-model-list\n强制刷新服务商模型列表，更新 /model 并保存缓存。"
            }
            "agents" => {
                "/agents\n设置此会话各代理角色的模型，恢复会话后仍保留。\n\n/agents <swarm|review|judge|memory|ambient>\n选择角色。使用全局默认值可清除会话覆盖；inherit 可忽略全局固定模型。\n\n/agents default [role]\n修改所有未设置会话覆盖的全局默认值。ambient 会话选项适用于由此会话启动的定时任务。"
            }
            "swarm-prompt" => {
                "/swarm-prompt\nOpen the active swarm routing prompt in $VISUAL or $EDITOR.\n\nJcode uses a nonblank project override at ./.jcode/swarm-prompt.md when present, then ~/.jcode/swarm-prompt.md, then the built-in default. If no editable override exists, this command creates the global file from the built-in default. Restart or reload Jcode after editing because running agent tool registries cache the prompt."
            }
            "subagent" => {
                "/subagent <prompt>\nLaunch a subagent immediately.\n\nOptional flags:\n  --type <kind>         sets the subagent type (default general)\n  --model <name>        overrides the subagent model for this run\n  --continue <id>       resumes an existing subagent session"
            }
            "observe" => {
                "/observe\n切换侧栏观察模式。仅显示最新加入上下文的工具调用或结果，不保存到磁盘。\n\n/observe on\n启用并聚焦观察页。\n\n/observe off\n关闭。\n\n/observe status\n查看状态。"
            }
            "todos" | "todo" => {
                "/todos\n在对话中显示待办卡片，随任务进度更新；再次执行可关闭。\n\n/todos panel\n切换侧栏待办页。\n\n/todos pin\n将待办固定在对话顶部，设置保存为 display.pin_todos。\n\n/todos on 或 off\n开启或关闭侧栏待办。\n\n/todos status\n查看卡片、侧栏和固定状态。"
            }
            "splitview" | "split-view" => {
                "/splitview\n在侧栏同步显示此会话，可独立滚动查看旧内容。\n\n/splitview on 或 off\n开启或关闭。\n\n/splitview status\n查看状态。"
            }
            "btw" => {
                "/btw <question>\nAsk a side question without derailing the current session.\n\nForks (splits) the session into a new window with the full conversation cloned, and the forked session starts by answering the question. The original session keeps working uninterrupted."
            }
            "git" => {
                "/git 或 /git status\n在会话工作目录运行 git status --short --branch，查看分支和文件状态。"
            }
            "commit" => {
                "/commit\n让代理检查当前修改，按相关文件或代码块分组提交，保留无关改动并进行适当验证。完成后报告提交及尚未提交的内容。"
            }
            "merge" => {
                "/merge\nAsk the agent to merge the current branch into main/master, then leave HEAD on that destination branch.\n\nRequires a clean worktree and an attached branch. If both main and master exist, use the unambiguous configured remote default or ask. The agent validates the changes, uses a normal merge (fast-forward when possible), and stops on conflicts, aborting its own merge and returning to the source branch when safe. Uncommitted changes are never auto-committed or stashed. Nothing is pushed and no branches are deleted."
            }
            "merge-remote-release" => {
                "/merge-remote-release\nSame as /merge, then /remote-release from the destination main/master branch. Leaves HEAD on that branch.\n\nRequires a clean worktree. Stops the entire workflow on conflicts, failed validation, an ambiguous destination, or already being on the destination branch. Uncommitted work is never auto-committed or stashed before merging. After a verified merge, pushes normally and follows the repository's remote release conventions. In Jcode, runs scripts/quick-release.sh --remote so release artifacts are built remotely. Never force-pushes, moves existing tags, deletes branches, or bypasses remote release gates. Reports the merge, validation, version, and remote release status."
            }
            "commit-push" | "commit-and-push" => {
                "/commit-push\n先按 /commit 的方式整理提交，再推送到跟踪分支；没有上游时使用 git push -u。不会强制推送或重写远端历史。"
            }
            "fast-release" | "cut-release" | "commit-push-release" => {
                "/fast-release\nSame as /commit-push, then publish the release as quickly as possible from the local Linux machine.\n\nThe agent picks the semver bump and first runs scripts/quick-release.sh --prepare-fast before changing Cargo.toml. This refreshes and records the warm target/selfdev Linux binary without invalidating the cache for a version change. The agent then makes one release-metadata commit containing Cargo.toml, Cargo.lock, and the changelog, pushes it, and runs scripts/quick-release.sh --fast-local. That command wraps the prepared binary with the release identity, publishes Linux and the GitHub release immediately, and lets CI replace it with the portable Linux build while adding every other platform and final signoff assets. /cut-release is a compatibility alias."
            }
            "fast-macos-release" => {
                "/fast-macos-release\nSame as /commit-push, but prepare and publish macOS arm64 as quickly as possible from the local Linux machine.\n\nThe agent first runs scripts/quick-release.sh --prepare-fast-macos before changing Cargo.toml. This cross-builds macOS arm64 with the future release identity and records its source commit and checksum. After the release-metadata commit, scripts/quick-release.sh --fast-macos-local validates and publishes that asset immediately. CI then replaces it with the signoff build and adds macOS Intel and every other platform. Requires osxcross."
            }
            "remote-release" => {
                "/remote-release\nSame as /commit-push, then push the release tag without running any local build.\n\nThe agent picks the semver bump, updates Cargo.toml/Cargo.lock and the changelog, commits and pushes, then runs scripts/quick-release.sh --remote. GitHub Actions builds, signs, checksums, and publishes every platform; the release remains a draft until the remote gates pass."
            }
            "triage" => {
                "/triage [focus]\nTriage open GitHub issues for the current repo, then autonomously fix the safe ones.\n\nThe agent lists untriaged issues with gh, classifies each (auto-fix, needs-info, needs-human, duplicate, question), applies existing labels, fixes and verifies the clear-cut bugs, and reports back with a summary table. Every public comment is clearly signed as the Jcode agent, and issues are never closed as wontfix/invalid without your confirmation.\n\nOptional focus text narrows the triage, for example /triage only crash reports."
            }
            "catchup" => {
                "/catchup\nOpen the Catch Up picker for finished sessions that need attention.\n\n/catchup next\nTeleport to the next session needing attention and open a Catch Up brief in the side panel.\n\n/catchup list\nAlias for opening the picker."
            }
            "back" => {
                "/back\nReturn to the previous session you came from via Catch Up.\n\nWorks after a /catchup next jump or after selecting a session from the Catch Up picker."
            }
            "subagent-model" => {
                "/subagent-model\nShow the current subagent model policy for this session.\n\n/subagent-model <name>\nPin a fixed model for future subagents in this session.\n\n/subagent-model inherit\nReset to using the current active model."
            }
            "autoreview" => {
                "/autoreview\nShow autoreview status for this session.\n\n/autoreview on\nEnable end-of-turn autoreview for this session.\n\n/autoreview off\nDisable autoreview for this session.\n\n/autoreview now\nLaunch a headed reviewer immediately in a new window."
            }
            "autojudge" => {
                "/autojudge\nShow autojudge status for this session.\n\n/autojudge on\nEnable end-of-turn autojudge for this session. The autojudge acts like a completion manager: it tells the parent agent either to continue with specific next steps or that it is fine to stop.\n\n/autojudge off\nDisable autojudge for this session.\n\n/autojudge now\nLaunch a headed autojudge immediately in a new window."
            }
            "review" => {
                "/review\nLaunch a one-shot headed review session immediately.\n\nThe reviewer will DM this session when done. If OpenAI ChatGPT OAuth is available, it prefers gpt-5.5."
            }
            "judge" => {
                "/judge\nLaunch a one-shot headed judge session immediately.\n\nThe judge will DM this session when done. If OpenAI ChatGPT OAuth is available, it prefers gpt-5.5."
            }
            "effort" => {
                "/effort\nShow current effort.\n\n/effort <level>\nSet effort (none|minimal|low|medium|high|xhigh|max|swarm|swarm-deep). Which levels apply depends on the model. The swarm rungs [Beta] turn on swarm orchestration (swarm: light fan-out, swarm-deep: deep task graph). Root reasoning is configurable via [agents] swarm_root_effort and swarm_deep_root_effort (both default to max).\n\nAlso: {effort_keys} to cycle."
            }
            "fast" => {
                "/fast\nShow whether fast mode is enabled, plus the saved default.\n\n/fast on\nEnable fast mode (service_tier = priority) for the current session.\n\n/fast ultra\nEnable OpenAI Ultrafast (service_tier = ultrafast) on models that support it, such as GPT-6 Astra.\n\n/fast off\nDisable fast mode for the current session.\n\nAlt+Up / Alt+Down cycle Standard -> Fast -> Ultrafast (configure speed_increase / speed_decrease).\n\n/fast status\nShow current fast-mode status.\n\n/fast default on\nSave fast mode as the default on startup.\n\n/fast default off\nSave fast mode as the default off on startup.\n\n/fast default status\nShow the saved fast-mode default."
            }
            "memory" => "/memory [on|off|status]\nToggle memory features for this session.",
            "log" => {
                "/log mark [note]\nWrite a distinctive JCODE_LOG_MARK line to ~/.jcode/logs/jcode-YYYY-MM-DD.log with the current session, provider, model, working directory, and optional note. Use this to mark a spot for agents to inspect later."
            }
            "goals" => {
                "/goals\nOpen the goals overview in the side panel.\n\n/goals resume\nResume the most relevant active goal for this session/project.\n\n/goals show <id>\nOpen a specific goal in the side panel."
            }
            "swarm" => "/swarm [on|off|status]\nToggle swarm features for this session.",
            "overnight" => {
                "/overnight <hours>[h|m] [mission]\nStart one overnight coordinator with a target wake/report time. The coordinator prioritizes verifiable, low-risk work, maintains structured logs, updates review notes, and generates a review HTML page.\n\n/overnight status\nShow the latest overnight run status.\n\n/overnight log\nShow recent overnight events.\n\n/overnight review\nOpen the generated review page.\n\n/overnight cancel\nRequest cancellation after the current coordinator turn reaches a safe boundary."
            }
            "voice" => {
                "/voice\nBuilt-in voice input, like Jcode Desktop. Run /voice or press Ctrl+Space to start recording, press again to send. On terminals that report key releases, hold Ctrl+Space and let go to send. Esc cancels.\n\nThe live transcript and a level meter show in the status line. The final text is sent to the agent wrapped in <transcription> tags, and anything already typed in the composer stays there.\n\nSetup:\n  Nari API key  NARI_API_KEY or ~/.config/jcode/nari.env\n  microphone    auto-detects pw-record, parecord, arecord, rec, or ffmpeg\n\nConfigure in ~/.jcode/config.toml:\n  [keybindings] voice_input = \"ctrl+space\"\n  [dictation]   recorder = \"<command printing raw s16le mono 16 kHz>\"\n                vocabulary = [\"names\", \"terms\"]"
            }
            "dictate" | "dictation" => {
                "/dictate\nRun the configured external speech-to-text command and inject the transcript into jcode.\n\nConfigure [dictation] in ~/.jcode/config.toml:\n  command       shell command that prints transcript to stdout,\n                for example ~/.local/bin/my-whisper-script --grammar-target code\n  mode          insert|append|replace|send\n  key           optional hotkey (for example alt+;)\n  timeout_secs  max wait time"
            }
            "poke" => {
                "/poke [on|off|status]\nPoke the model to resume when it has stopped with incomplete todos.\n\n\
                Auto-poke now starts enabled by default, and Ctrl+P toggles it on/off.\n\
                Set auto_poke = false under [features] in ~/.jcode/config.toml to start with it disabled.\n\
                /poke or /poke on arms auto-poke and immediately pokes if work remains.\n\
                /poke off disarms auto-poke and clears any queued poke follow-ups.\n\
                /poke status shows whether auto-poke is currently armed.\n\
                If a turn is currently running, the poke is queued and sent right after that turn finishes.\n\
                Injects a reminder with the number of incomplete todos and prompts the model to either\n\
                finish the work, update the todo list to reflect what is done, or ask for user input if genuinely blocked."
            }
            "transfer" => {
                "/transfer\nCompact the current session into a summary-only handoff, copy the current todo list to a fresh session, and open that transferred session in a new window.\n\nIf a turn is currently running, jcode first soft-pauses the current session at the next safe point, then performs the transfer."
            }
            "plan" => {
                "/plan [goal]\nDraft a plan without implementing anything. The model inspects the repo, then presents a structured plan (Goal, Scope, Approach, Validation, Open questions) as a dedicated plan card in the conversation.\n\nNothing is edited: it stops after presenting the plan. Once you approve, it converts the plan into a todo list and starts the work.\n\n/plan with no goal plans the task currently in focus."
            }
            "improve" => {
                "/improve [focus]\nStart an autonomous repo-improvement loop. The model inspects the project, writes a ranked todo list, implements the highest-leverage safe improvements, validates them, then keeps going until further work has diminishing returns.\n\n/improve plan [focus]\nGenerate a ranked improve todo list only, without editing files.\n\n/improve resume\nResume the last saved improve mode for this session using the current improve todos.\n\n/improve status\nShow the inferred status of the current improve run and todo batch.\n\n/improve stop\nAsk the model to stop after the next safe point, update todos, and summarize remaining work."
            }
            "refactor" => {
                "/refactor [focus]\nStart a refactor loop aimed at moving the repo toward a practical 10/10. The main agent inspects the project, writes a ranked refactor todo list, implements the best safe refactors itself, validates each batch, and asks one independent read-only subagent to review each meaningful batch before continuing.\n\n/refactor plan [focus]\nGenerate a ranked refactor todo list only, without editing files.\n\n/refactor resume\nResume the last saved refactor mode for this session using the current refactor todos.\n\n/refactor status\nShow the inferred status of the current refactor run and todo batch.\n\n/refactor stop\nAsk the model to stop after the next safe point, update todos, and summarize remaining work."
            }
            "reload" => "/reload\n重新加载最新可用程序，保留会话，不重新构建。",
            "restart" => {
                "/restart\n使用当前程序重启并保留会话。修改配置、MCP 服务器或环境变量后可使用此命令。"
            }
            "rebuild" => {
                "/rebuild\n在后台执行 git pull --ff-only、发布构建和测试；完成后自动重新加载。构建期间仍可使用 JCode。"
            }
            "selfdev" => {
                "/selfdev\n在新终端打开自开发会话。\n\n/selfdev <prompt>\n同时发送初始提示词。\n\n/selfdev status\n查看自开发与构建状态。"
            }
            "fork" | "split" => {
                "/fork\n复制完整对话到新终端窗口，两个会话从同一点继续；tmux 中默认打开右侧面板。\n\n/fork <prompt>\n新会话先回答指定问题，原会话继续工作。\n\n/split\n/fork 的别名。"
            }
            "resume" | "sessions" => {
                "/resume\n打开历史会话选择器，可搜索会话、预览历史并恢复所选项。默认 Enter 在当前终端恢复，Ctrl+Enter 打开新终端；可通过 keybindings.session_picker_enter 交换。{resume_shortcut}\n\nEsc 返回当前会话。"
            }
            "info" => "/info\n查看会话信息和 Token 用量。",
            "context" => {
                "/context\n查看完整上下文快照，包括提示词组成、压缩状态、模型和服务商、排队工作、待办及侧栏状态。"
            }
            "usage" => "/usage\n查询已连接服务商的用量窗口和重置时间。仅显示服务商返回的真实数据。",
            "subscription" => {
                "/subscription\nCompatibility alias for /hosted status. Shows hosted-model usage, your monthly spending limit, billing status, and router configuration."
            }
            "subscribe" | "hosted" => {
                "/hosted\nUse Jcode hosted models without a subscription: choose a monthly spending limit, receive milestone warnings without throttling, and pay in progressively larger tranches. Sign in once with /login jcode.\n\n/hosted status\nShow usage and your current spending limit.\n\n/subscribe\nCompatibility alias for /hosted."
            }
            "version" => "/version\n查看 JCode 版本及构建信息。",
            "changelog" => "/changelog\n查看此程序内嵌的更新记录。",
            "quit" => "/quit\n退出 JCode。",
            "config" => {
                "/config\n查看当前配置。\n\n/config init\n创建默认配置文件。\n\n/config edit\n使用 $EDITOR 打开配置文件。"
            }
            "alignment" => {
                "/alignment\n查看当前对齐方式和已保存的默认值。\n\n/alignment centered\n保存并立即应用居中对齐。\n\n/alignment left\n保存并立即应用左对齐。\n\nAlt+C 仅切换当前会话的对齐方式。"
            }
            "compact-notifications" => {
                "/compact-notifications\nShow whether swarm/file-activity notifications are compact.\n\n/compact-notifications on\nCollapse file-activity notifications to a single line (path · summary), dropping the intent and diff preview.\n\n/compact-notifications off\nRestore the full multi-line notification cards."
            }
            "show-agentgrep-output" => {
                "/show-agentgrep-output\nShow whether full agentgrep search output renders inline in the transcript.\n\n/show-agentgrep-output on\nRender the full agentgrep search results inline beneath each agentgrep call instead of just the one-line summary.\n\n/show-agentgrep-output off\nShow only the compact one-line agentgrep summary."
            }
            "tool-call-details" => {
                "/tool-call-details\nShow whether the dimmed technical detail (command, path, args) renders next to the model-provided intent on tool rows.\n\n/tool-call-details on\nShow the technical detail after the intent, e.g. `bash · Run tests · $ cargo test`.\n\n/tool-call-details off\nShow only the intent on tool rows that have one. Rows without an intent still show the technical detail, and error summaries always render."
            }
            "auth" | "login" => {
                "/auth\n查看所有服务商的认证状态。\n\n/login\n打开服务商登录选择器。\n\n/login <provider>\n直接开始指定服务商的登录。\n\n/login jcode\n使用 JCode 托管模型并在浏览器设置月度消费额度，无需在终端粘贴 API 密钥。"
            }
            "reset" => {
                "/reset 或 /reset usage limits openai\n查看可用的 OpenAI 重置次数，暂不消耗。重复执行显示同一待确认操作。\n\n/reset usage limits openai confirm\n使用已查看的重置次数，无法撤销。结果不确定时可重复确认以重试同一请求。\n\n/reset usage limits openai cancel\n清除待确认操作；已提交的重置无法取消。"
            }
            "account" | "accounts" => {
                "/account\n打开 Claude 和 OpenAI 账户选择器，查看已保存账户并添加或替换。\n\n/account <provider>\n按服务商筛选。\n\n/account <provider> settings\n查看账户和配置详情。\n\n/account <provider> login\n登录或刷新凭据。\n\n/account claude add 或 /account openai add\n添加下一个编号的 OAuth 账户。\n\n/account <provider> switch <label>\n切换当前账户。\n\n/account <provider> remove <label>\n删除已保存账户。\n\n/account default-provider <provider|auto>\n设置后续会话的默认服务商。\n\n/account default-model <model|clear>\n设置或清除默认模型。\n\nOpenAI 还支持 transport、effort、fast 设置；自定义服务商支持 api-base、api-key-name、env-file、default-model。"
            }
            "save" => {
                "/save\n收藏当前会话，使其显示在 /resume 的已收藏分组中。\n\n/save <label>\n使用自定义标签收藏会话。"
            }
            "rename" => {
                "/rename <session name>\nSet a custom display title for the current session. This updates the window title and /resume display.\n\n/rename --clear\nClear the custom name and return to the generated session title."
            }
            "unsave" => "/unsave\n取消收藏当前会话。",
            "client-reload" if self.is_remote => {
                "/client-reload\nForce client binary reload in remote mode."
            }
            "server-reload" if self.is_remote => {
                "/server-reload\nForce server binary reload in remote mode."
            }
            "continue" | "resumeall" | "resume-all" if self.is_remote => {
                "/continue\nContinue every interrupted live session that would auto-resume on a reload.\n\nThe server walks all currently-live sessions and, for each idle one that still owes the model a reply (a turn that errored or was interrupted mid-generation), injects the standard \"continue where you left off\" reminder so it picks back up. Sessions that are busy, fresh, or already complete are left untouched.\n\nAlias: /resumeall."
            }
            _ => return None,
        };
        let help = help.replace(
            "{effort_keys}",
            &crate::tui::keybind::effort_switch_keys_label(),
        );
        let resume_shortcut = match crate::tui::keybind::load_open_resume_key().label {
            Some(label) => format!("也可按 {label} 直接打开。"),
            None => String::new(),
        };
        let help = help.replace("{resume_shortcut}", &resume_shortcut);
        // Mac keyboards have no "Alt" key; show the ⌥ keycap instead.
        let help = help.replace(
            "Alt+",
            &format!("{}+", jcode_tui_core::keybind::alt_label()),
        );
        Some(help)
    }
}
