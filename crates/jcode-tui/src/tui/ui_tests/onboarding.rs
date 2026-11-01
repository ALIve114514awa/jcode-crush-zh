use super::*;
use ratatui::backend::TestBackend;
use ratatui::{Terminal, layout::Rect};

/// Render the onboarding welcome screen for the given state at the given size
/// and return the flattened text of the whole buffer.
fn render_onboarding(state: &TestState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| {
            let area = Rect::new(0, 0, width, height);
            crate::tui::ui::onboarding::draw_onboarding_welcome(frame, state, area);
        })
        .expect("failed to draw onboarding");

    let buf = terminal.backend().buffer();
    let mut lines = Vec::with_capacity(height as usize);
    for y in 0..height {
        let line = crate::tui::test_harness::buffer_line_text(buf, y);
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn onboarding_state() -> TestState {
    TestState {
        onboarding_preview: true,
        suggestions: vec![
            ("Log in to get started".to_string(), "/login".to_string()),
            (
                "Build a small CLI tool".to_string(),
                "build a CLI".to_string(),
            ),
        ],
        ..Default::default()
    }
}

#[test]
fn onboarding_welcome_shows_title_and_suggestions_without_telemetry_blurb() {
    let state = onboarding_state();
    let text = render_onboarding(&state, 80, 30);

    assert!(
        !text.contains("anonymous usage statistics"),
        "first page should not carry the telemetry blurb:\n{text}"
    );
    assert!(
        !text.contains("JCODE_NO_TELEMETRY=1"),
        "first page should not carry the telemetry opt-out hint:\n{text}"
    );
    assert!(
        text.contains("欢迎使用 JCode"),
        "welcome title should be rendered:\n{text}"
    );
    assert!(
        text.contains("Log in to get started"),
        "login suggestion should be rendered:\n{text}"
    );
    assert!(
        text.contains("Build a small CLI tool"),
        "secondary suggestion should be rendered:\n{text}"
    );
    assert!(
        text.contains("按 1-2 选择，或直接输入内容开始"),
        "numeric hint should reflect suggestion count:\n{text}"
    );
}

#[test]
fn onboarding_welcome_login_suggestion_shows_typed_command() {
    let state = onboarding_state();
    let text = render_onboarding(&state, 80, 30);
    assert!(
        text.contains("（输入 /login）"),
        "login suggestion should hint the slash command:\n{text}"
    );
}

#[test]
fn onboarding_welcome_renders_on_tiny_area_without_panicking() {
    // Below the donut/full-treatment threshold: should fall back gracefully.
    // The title may be truncated at narrow widths, so only assert its prefix.
    let state = onboarding_state();
    let text = render_onboarding(&state, 20, 5);
    assert!(
        text.contains("欢迎使用 JCode"),
        "minimal fallback should still show the title:\n{text}"
    );
}

#[test]
fn onboarding_welcome_centers_within_tall_area() {
    // A tall area should leave blank padding above the telemetry header.
    let state = onboarding_state();
    let text = render_onboarding(&state, 80, 40);
    let first_nonblank = text
        .lines()
        .position(|line| !line.trim().is_empty())
        .expect("expected some content");
    assert!(
        first_nonblank > 0,
        "content should be vertically padded from the top:\n{text}"
    );
}
