use super::{
    accent_color, ai_color, ai_text, asap_color, clear_area, dim_color, get_grouped_changelog,
    header_icon_color, header_name_color, header_session_color, pending_color, queued_color,
    record_chat_overlay_copy_snapshot, rgb, tool_color, user_bg, user_color, user_text,
};
use crate::tui::TuiState;
use crate::tui::info_widget::WidgetPlacement;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use super::selection_highlight::highlight_line_selection;

pub(super) fn draw_changelog_overlay(
    frame: &mut Frame,
    area: Rect,
    scroll: usize,
    app: &dyn TuiState,
) {
    clear_area(frame, area);

    let groups = get_grouped_changelog();
    let mut lines: Vec<Line<'static>> = Vec::new();

    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "暂无更新记录。",
            Style::default().fg(dim_color()),
        )));
    } else {
        for group in &groups {
            let heading = match &group.released_at {
                Some(released_at) => format!("  {} · {}", group.version, released_at),
                None => format!("  {}", group.version),
            };
            lines.push(Line::from(Span::styled(
                heading,
                Style::default()
                    .fg(rgb(200, 200, 220))
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            for entry in &group.entries {
                lines.push(Line::from(vec![
                    Span::styled("    • ", Style::default().fg(dim_color())),
                    Span::styled(entry.clone(), Style::default().fg(rgb(170, 170, 185))),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    let total_lines = lines.len();
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = scroll.min(max_scroll);

    let scroll_info = if total_lines > visible_height {
        let pct = if max_scroll > 0 {
            (scroll * 100) / max_scroll
        } else {
            100
        };
        format!(" {}% ", pct)
    } else {
        String::new()
    };

    let title = format!(" 更新记录 {} ", scroll_info);
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(rgb(200, 200, 220))
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " Esc 关闭 · 拖选后松开复制 · 滚轮/j/k 滚动 ",
            Style::default().fg(dim_color()),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(dim_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let visible_end = scroll
        .saturating_add(inner.height as usize)
        .min(total_lines);

    // Register the rendered lines so the shared copy-selection machinery can map
    // mouse drags to text and highlight + copy the selection, exactly like the
    // chat viewport. Without this, mouse capture would block native terminal
    // selection and there would be no way to copy from the overlay.
    record_chat_overlay_copy_snapshot(&lines, scroll, visible_end, inner);

    let mut visible_lines: Vec<Line<'static>> =
        lines.get(scroll..visible_end).unwrap_or(&[]).to_vec();

    if let Some(range) = app.copy_selection_range().filter(|range| {
        range.start.pane == crate::tui::CopySelectionPane::Chat
            && range.end.pane == crate::tui::CopySelectionPane::Chat
    }) {
        let (start, end) = if (range.start.abs_line, range.start.column)
            <= (range.end.abs_line, range.end.column)
        {
            (range.start, range.end)
        } else {
            (range.end, range.start)
        };
        for abs_idx in start.abs_line.max(scroll)..=end.abs_line.min(visible_end.saturating_sub(1))
        {
            let rel_idx = abs_idx.saturating_sub(scroll);
            if let Some(line) = visible_lines.get_mut(rel_idx) {
                let start_col = if abs_idx == start.abs_line {
                    start.column
                } else {
                    0
                };
                let end_col = if abs_idx == end.abs_line {
                    end.column
                } else {
                    line.width()
                };
                *line = highlight_line_selection(line, start_col, end_col);
            }
        }
    }

    frame.render_widget(Paragraph::new(visible_lines), inner);
}

pub(super) fn draw_commit_log_overlay(
    frame: &mut Frame,
    area: Rect,
    overlay: &crate::tui::app::CommitLogOverlay,
) {
    clear_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" 提交记录：{} ", overlay.title))
        .title_bottom(Line::from(Span::styled(
            " j/k 移动 · d/u 翻页 · g/G 顶部/底部 · c 复制哈希 · q/Esc 关闭 ",
            Style::default().fg(dim_color()),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if let Some(error) = &overlay.error {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {error}"),
                Style::default().fg(rgb(240, 110, 110)),
            ))),
            inner,
        );
        return;
    }
    if overlay.entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " 暂无提交。",
                Style::default().fg(dim_color()),
            ))),
            inner,
        );
        return;
    }

    let height = inner.height as usize;
    let selected = overlay.scroll.min(overlay.entries.len() - 1);
    // Keep the selected row roughly centered once it moves past the middle.
    let start = selected
        .saturating_sub(height / 2)
        .min(overlay.entries.len().saturating_sub(height));

    let hash_style = Style::default().fg(rgb(200, 180, 120));
    let date_style = Style::default().fg(dim_color());
    let author_style = Style::default().fg(rgb(130, 170, 230));
    let refs_style = Style::default()
        .fg(accent_color())
        .add_modifier(Modifier::BOLD);
    let subject_style = Style::default().fg(rgb(220, 220, 230));
    let selected_bg = rgb(45, 50, 65);

    let lines: Vec<Line<'static>> = overlay
        .entries
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(index, entry)| {
            let mut spans = vec![
                Span::styled(format!(" {} ", entry.hash), hash_style),
                Span::styled(format!("{} ", entry.date), date_style),
                Span::styled(format!("{} ", entry.author), author_style),
            ];
            if !entry.refs.is_empty() {
                spans.push(Span::styled(format!("({}) ", entry.refs), refs_style));
            }
            spans.push(Span::styled(entry.subject.clone(), subject_style));
            let line = Line::from(spans);
            if index == selected {
                line.style(Style::default().bg(selected_bg))
            } else {
                line
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw_help_overlay(frame: &mut Frame, area: Rect, scroll: usize, app: &dyn TuiState) {
    clear_area(frame, area);

    let section_style = Style::default()
        .fg(accent_color())
        .add_modifier(Modifier::BOLD);
    let cmd_style = Style::default().fg(rgb(230, 230, 240));
    let desc_style = Style::default().fg(rgb(150, 150, 165));
    let key_style = Style::default().fg(rgb(200, 180, 120));
    let sep_style = Style::default().fg(rgb(50, 50, 55));

    let mut lines: Vec<Line<'static>> = Vec::new();

    let separator = || -> Line<'static> {
        Line::from(Span::styled(
            "  ─────────────────────────────────────────────────",
            sep_style,
        ))
    };

    let help_entry = |cmd: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled("    ", Style::default()),
            Span::styled(cmd.to_string(), cmd_style),
            Span::styled("  ", Style::default()),
            Span::styled(desc.to_string(), desc_style),
        ])
    };

    let alt = jcode_tui_core::keybind::alt_chord;
    let key_entry = |key: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled("    ", Style::default()),
            Span::styled(format!("{:<22}", key), key_style),
            Span::styled(desc.to_string(), desc_style),
        ])
    };

    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  常用命令", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/help", "显示帮助"));
    lines.push(help_entry("/help <command>", "查看指定命令的详细帮助"));
    lines.push(help_entry("/model", "选择或切换模型"));
    lines.push(help_entry("/model <name>", "切换到指定模型"));
    lines.push(help_entry(
        "/provider-test-coverage",
        "查看当前服务商和模型的实测记录",
    ));
    lines.push(help_entry("/agents", "配置各代理角色使用的模型"));
    lines.push(help_entry("/swarm-prompt", "在编辑器中打开代理协作提示词"));
    lines.push(help_entry(
        "/effort <level>",
        "设置推理强度（none|minimal|low|medium|high|xhigh|max|swarm|swarm-deep）",
    ));
    lines.push(help_entry(
        "/fast [on|off|status|default ...]",
        "切换快速模式",
    ));
    lines.push(help_entry(
        "/transport <mode>",
        "设置连接方式（auto|https|websocket）",
    ));
    lines.push(help_entry(
        "/alignment [status|centered|left]",
        "查看或保存文字对齐方式",
    ));
    lines.push(help_entry(
        "/compact-notifications [status|on|off]",
        "将协作和文件活动通知折叠为一行",
    ));
    lines.push(help_entry(
        "/show-agentgrep-output [status|on|off]",
        "在对话中显示完整搜索结果",
    ));
    lines.push(help_entry("/config", "查看当前配置"));
    lines.push(help_entry("/config init", "创建默认配置文件"));
    lines.push(help_entry("/config edit", "使用 $EDITOR 打开配置"));
    lines.push(help_entry("/voice", "语音输入：说话后发送（Ctrl+Space）"));
    lines.push(help_entry("/dictate", "运行已配置的外部听写工具"));
    lines.push(help_entry("/git [status]", "查看分支及工作区状态"));
    lines.push(help_entry("/context", "查看完整会话上下文"));
    lines.push(help_entry("/skills", "查看已加载技能和推荐技能"));
    lines.push(help_entry("/info", "查看会话信息及 Token 用量"));
    lines.push(help_entry("/keys", "查看与终端或系统冲突的快捷键"));
    lines.push(help_entry(
        "/reset usage limits openai",
        "查看可用重置次数并确认或取消",
    ));
    lines.push(help_entry("/usage", "查看已连接服务商的用量限制"));
    lines.push(help_entry("/support", "生成附带诊断信息的支持邮件"));
    lines.push(help_entry("/version", "查看版本及构建信息"));
    lines.push(help_entry("/changelog", "查看此版本的更新记录"));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  会话", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/clear", "清空对话并开始新会话"));
    lines.push(help_entry("/compact", "总结旧消息以释放上下文"));
    lines.push(help_entry("/rewind", "查看历史编号，使用 /rewind N 回退"));
    lines.push(help_entry("/fix", "尝试恢复无法继续的会话"));
    lines.push(help_entry(
        "/poke",
        "提醒模型继续未完成待办（on/off/status）",
    ));
    lines.push(help_entry("/plan [goal]", "生成计划卡片，暂不修改文件"));
    lines.push(help_entry("/improve", "持续自主改进仓库"));
    lines.push(help_entry("/improve resume", "继续上次保存的改进计划"));
    lines.push(help_entry("/refactor", "执行带独立审查的重构"));
    lines.push(help_entry("/refactor resume", "继续上次保存的重构计划"));
    lines.push(help_entry(
        "/splitview [on|off|status]",
        "在侧栏同步显示当前对话",
    ));
    lines.push(help_entry(
        "/fork [prompt]",
        "在新窗口分支会话（别名：/split）",
    ));
    lines.push(help_entry("/transfer", "以压缩上下文和待办列表开始新会话"));
    lines.push(help_entry(
        "/workspace [status|on|off|add]",
        "启用并管理会话工作区",
    ));
    lines.push(help_entry(
        "/catchup [next|list]",
        "切换到已完成会话并显示摘要",
    ));
    lines.push(help_entry("/back", "返回查看摘要前的会话"));
    lines.push(help_entry("/resume", "浏览并恢复历史会话"));
    lines.push(help_entry("/active", "管理运行中的会话并查看状态"));
    lines.push(help_entry(
        "/catchup [next]",
        "切换到已完成会话并在侧栏显示摘要",
    ));
    lines.push(help_entry("/back", "返回上一个摘要会话"));
    lines.push(help_entry("/save [label]", "收藏会话，以便 /resume 恢复"));
    lines.push(help_entry(
        "/rename <name>|--clear",
        "设置或清除当前会话名称",
    ));
    lines.push(help_entry("/unsave", "取消收藏当前会话"));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  记忆与代理协作", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/memory [on|off]", "开启或关闭记忆功能"));
    lines.push(help_entry("/test [claim]", "执行分层验证并生成证据"));
    lines.push(help_entry("/initiatives", "查看或恢复长期任务"));
    lines.push(help_entry("/swarm [on|off]", "开启或关闭代理协作"));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  登录与账户", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/auth", "查看认证状态"));
    lines.push(help_entry(
        "/login [provider]",
        "交互登录或直接指定服务商登录",
    ));
    lines.push(help_entry("/account", "选择 Claude 或 OpenAI 账户"));
    lines.push(help_entry("/subscription", "查看 JCode 订阅状态"));
    lines.push(help_entry("/subscribe", "了解 JCode 订阅方式"));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  系统", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/reload", "重新加载可用的新版本"));
    lines.push(help_entry("/restart", "使用当前程序重启"));
    lines.push(help_entry("/rebuild", "完整更新（拉取源码、构建并测试）"));
    if app.is_remote_mode() {
        lines.push(help_entry("/client-reload", "强制重新加载客户端"));
        lines.push(help_entry("/server-reload", "强制重新加载服务器"));
        lines.push(help_entry("/continue", "继续所有等待自动恢复的会话"));
    }
    lines.push(help_entry("/debug-visual", "启用终端界面调试"));
    lines.push(help_entry("/quit", "退出 JCode"));

    // The sections above are hand-curated for ordering, but they drift as
    // commands are added. Anything registered and not already shown gets listed
    // here so no working command is invisible in /help.
    let shown: std::collections::HashSet<String> = lines
        .iter()
        .filter_map(|line| line.spans.get(1).map(|span| span.content.to_string()))
        .map(|cmd| {
            cmd.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    let uncovered: Vec<(&str, &str)> = crate::tui::app::registered_command_entries()
        .filter(|(name, _)| !shown.contains(*name))
        .collect();
    if !uncovered.is_empty() {
        lines.push(Line::from(""));
        lines.push(separator());
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  更多命令", section_style)));
        lines.push(Line::from(""));
        for (name, desc) in uncovered {
            lines.push(help_entry(name, desc));
        }
    }

    let skills = app.available_skills();
    if !skills.is_empty() {
        lines.push(Line::from(""));
        lines.push(separator());
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled("  技能", section_style)));
        lines.push(Line::from(""));
        for skill in &skills {
            lines.push(help_entry(&format!("/{}", skill), "启用技能"));
        }
    }

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  导航", section_style)));
    lines.push(Line::from(""));
    lines.push(key_entry("PageUp / PageDown", "滚动历史记录"));
    lines.push(key_entry("Up / Down", "输入框为空时滚动历史"));
    lines.push(key_entry(
        "Ctrl+J / Ctrl+K",
        "跳转到下一条或上一条用户消息（也可用 Ctrl+] / Ctrl+[）",
    ));
    lines.push(key_entry(
        "Ctrl+Shift+J / Ctrl+Shift+K",
        "向下或向上滚动一行",
    ));
    lines.push(key_entry(
        "Cmd/Super+K / J",
        "跳转到上一条或下一条用户消息（macOS）",
    ));
    lines.push(key_entry("Ctrl+1..4", "将侧栏宽度设为 25/50/75/100%"));
    lines.push(key_entry(
        "Ctrl+5..9",
        "按最近顺序跳转（5 表示倒数第 5 项）",
    ));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  图表与差异", section_style)));
    lines.push(Line::from(""));
    lines.push(key_entry(
        &crate::tui::keybind::side_panel_toggle_key_label(),
        "切换侧栏：分栏、全屏、隐藏",
    ));
    lines.push(key_entry(
        &crate::tui::keybind::diagram_pane_visibility_key_label(),
        "显示或隐藏图表面板",
    ));
    lines.push(key_entry(&alt("T"), "切换图表位置（侧边/顶部）"));
    lines.push(key_entry(&alt("Shift+I"), "显示或隐藏内嵌图片（保存设置）"));
    lines.push(key_entry("Ctrl+H / Ctrl+L", "切换焦点：对话、图表、差异"));
    lines.push(key_entry(
        "Ctrl+L / Cmd+L",
        "清屏并保留历史（无面板焦点时）",
    ));
    lines.push(key_entry("Ctrl+Left / Right", "切换图表（图表获得焦点时）"));
    lines.push(key_entry("h/j/k/l / arrows", "移动图表（获得焦点时）"));
    lines.push(key_entry("[ / ]", "缩放图表（获得焦点时）"));
    lines.push(key_entry("+ / -", "调整图表面板大小"));
    lines.push(key_entry("/diff", "切换差异模式：关闭、内嵌、固定、文件"));
    lines.push(key_entry(&alt("G"), "查看项目 Git 提交记录"));
    lines.push(key_entry("Shift+Tab", "切换收藏的模型"));
    lines.push(key_entry("Ctrl+O", "设为默认模型（模型选择器中）"));
    lines.push(key_entry("Ctrl+N", "收藏或取消收藏模型"));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  输入与编辑", section_style)));
    lines.push(Line::from(""));
    lines.push(key_entry("Ctrl+C / Ctrl+D", "退出（按两次确认）"));
    lines.push(key_entry("Ctrl+X", "剪切整行输入"));
    lines.push(key_entry("Ctrl+A", "复制当前可见对话及相邻上下文"));
    lines.push(key_entry("Ctrl+U", "清空输入行"));
    lines.push(key_entry("Ctrl+K", "删除至输入末尾"));
    lines.push(key_entry(
        &format!("{} / {}", alt("Backspace"), alt("Delete")),
        "删除上一个词",
    ));
    lines.push(key_entry("Cmd/Super+Backspace / Delete", "删除上一个词"));
    if cfg!(target_os = "macos") {
        // On macOS, Cmd+Left/Right default to effort cycling; Home/End and
        // Cmd+A/E still jump to the start/end of the input.
        lines.push(key_entry("Home / End", "移至输入开头或末尾"));
    } else {
        lines.push(key_entry("Cmd/Super+Left / Right", "移至输入开头或末尾"));
    }
    lines.push(key_entry("Cmd/Super+Z", "撤销输入修改"));
    lines.push(key_entry("Cmd/Super+X / V", "剪切输入或粘贴剪贴板"));
    lines.push(key_entry("Ctrl+S", "暂存或恢复输入"));
    lines.push(key_entry("Ctrl+Backspace", "删除上一个词"));
    lines.push(key_entry("Ctrl+B / Ctrl+F", "按词向左或向右移动"));
    lines.push(key_entry("Ctrl+Left / Right", "按词向左或向右移动"));
    lines.push(key_entry(
        &format!("Shift+Enter / {}", alt("Enter")),
        "插入换行",
    ));
    lines.push(key_entry(
        "Trailing \\ then Enter",
        "插入换行（可用 /terminal-setup 配置 Shift+Enter）",
    ));
    lines.push(key_entry(
        "Ctrl+Enter / Cmd+Enter",
        "处理时临时切换立即发送和排队",
    ));
    lines.push(key_entry("Ctrl+Up", "取回待发送消息进行编辑"));
    lines.push(key_entry("Ctrl+Tab / Ctrl+T", "切换排队模式"));
    lines.push(key_entry("Ctrl+R", "跨会话搜索输入历史"));
    lines.push(key_entry(
        &format!("Ctrl+V / {}", alt("V")),
        "粘贴文字或图片",
    ));
    lines.push(key_entry(&alt("A"), "快速复制可见对话及相邻上下文"));
    lines.push(key_entry(&alt("Y"), "切换对话选择和复制模式"));
    lines.push(key_entry(&alt("S"), "切换输入时的滚动锁定"));
    lines.push(key_entry("Ctrl+P", "切换未完成待办的自动提醒"));
    lines.push(key_entry(&alt("X"), "显示或关闭对话中的待办卡片"));
    lines.push(key_entry(
        &crate::tui::keybind::effort_switch_keys_label(),
        "切换推理和代理协作强度",
    ));
    if cfg!(target_os = "macos") {
        lines.push(key_entry(
            &alt("Left / Right"),
            &format!("按词移动（也可用 {} / {}）", alt("B"), alt("F")),
        ));
    }
    if let Some(label) = app.dictation_key_label() {
        lines.push(key_entry(&label, "运行已配置的听写工具"));
    }
    if let Some(label) = crate::tui::keybind::load_open_resume_key().label {
        lines.push(key_entry(&label, "打开历史会话选择器"));
    }
    if let Some(label) = crate::tui::keybind::load_new_terminal_key().label {
        lines.push(key_entry(&label, "在新终端中打开 JCode 会话"));
    }

    lines.push(Line::from(""));

    let total_lines = lines.len();
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = scroll.min(max_scroll);

    let scroll_info = if total_lines > visible_height {
        let pct = if max_scroll > 0 {
            (scroll * 100) / max_scroll
        } else {
            100
        };
        format!(" {}% ", pct)
    } else {
        String::new()
    };

    let title = format!(" 帮助 {} ", scroll_info);
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(rgb(200, 200, 220))
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " Esc 关闭 · 滚轮/j/k 滚动 · Space/PageUp 翻页 · /help <cmd> 详细帮助 ",
            Style::default().fg(dim_color()),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(dim_color()));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

pub(super) fn draw_model_status_overlay(
    frame: &mut Frame,
    area: Rect,
    scroll: usize,
    content: &str,
) {
    clear_area(frame, area);

    let title_style = Style::default()
        .fg(accent_color())
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(rgb(210, 210, 220));
    let dim_style = Style::default().fg(dim_color());

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled("  模型状态", title_style)));
    lines.push(Line::from(Span::styled(
        "  服务商和模型在 JCode 中的实测记录",
        dim_style,
    )));
    lines.push(Line::from(""));

    for raw in content.lines() {
        if let Some(title) = raw.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(format!("  {title}"), title_style)));
        } else if let Some(title) = raw.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(format!("  {title}"), title_style)));
        } else if raw.trim().is_empty() {
            lines.push(Line::from(""));
        } else {
            lines.push(Line::from(Span::styled(
                format!("  {raw}"),
                model_status_line_style(raw, text_style),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↑/↓ 滚动 · PgUp/PgDn 翻页 · c 复制报告 · q/Esc 关闭",
        dim_style,
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" /provider-test-coverage "),
        )
        .scroll((scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(paragraph, area);
}

fn model_status_line_style(raw: &str, default: Style) -> Style {
    // Reuse the same semantic classification the CLI uses so the TUI overlay
    // and `jcode provider-test-coverage` stay color-consistent.
    use crate::live_tests::CoverageLineStyle;
    match crate::live_tests::classify_provider_test_coverage_line(raw) {
        CoverageLineStyle::Title => Style::default()
            .fg(accent_color())
            .add_modifier(Modifier::BOLD),
        CoverageLineStyle::Pass => Style::default().fg(rgb(120, 220, 150)),
        CoverageLineStyle::Fail => Style::default().fg(rgb(240, 110, 110)),
        CoverageLineStyle::Warn => Style::default().fg(rgb(235, 190, 105)),
        CoverageLineStyle::Dim => Style::default().fg(dim_color()),
        CoverageLineStyle::Plain => default,
    }
}

pub(super) fn draw_debug_overlay(
    frame: &mut Frame,
    placements: &[WidgetPlacement],
    chunks: &[Rect],
) {
    if chunks.len() < 5 {
        return;
    }
    render_overlay_box(frame, chunks[0], "messages", Color::Red);
    render_overlay_box(frame, chunks[1], "queued", Color::Yellow);
    render_overlay_box(frame, chunks[2], "status", Color::Cyan);
    render_overlay_box(frame, chunks[3], "picker", Color::Magenta);
    render_overlay_box(frame, chunks[4], "input", Color::Green);
    if chunks.len() > 5 && chunks[5].height > 0 {
        render_overlay_box(frame, chunks[5], "donut", Color::Blue);
    }

    for placement in placements {
        let title = format!("widget:{}", placement.kind.as_str());
        render_overlay_box(frame, placement.rect, &title, Color::Magenta);
    }
}

fn render_overlay_box(frame: &mut Frame, area: Rect, title: &str, color: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(Span::styled(title.to_string(), Style::default().fg(color)));
    frame.render_widget(block, area);
}

pub(super) fn debug_palette_json() -> Option<serde_json::Value> {
    Some(serde_json::json!({
        "user_color": color_to_rgb(user_color()),
        "ai_color": color_to_rgb(ai_color()),
        "tool_color": color_to_rgb(tool_color()),
        "dim_color": color_to_rgb(dim_color()),
        "accent_color": color_to_rgb(accent_color()),
        "queued_color": color_to_rgb(queued_color()),
        "asap_color": color_to_rgb(asap_color()),
        "pending_color": color_to_rgb(pending_color()),
        "user_text": color_to_rgb(user_text()),
        "user_bg": color_to_rgb(user_bg()),
        "ai_text": color_to_rgb(ai_text()),
        "header_icon_color": color_to_rgb(header_icon_color()),
        "header_name_color": color_to_rgb(header_name_color()),
        "header_session_color": color_to_rgb(header_session_color()),
    }))
}

fn color_to_rgb(color: Color) -> Option<[u8; 3]> {
    match color {
        Color::Rgb(r, g, b) => Some([r, g, b]),
        Color::Indexed(n) if n >= 16 => {
            let (r, g, b) = crate::tui::color_support::indexed_to_rgb(n);
            Some([r, g, b])
        }
        _ => None,
    }
}
