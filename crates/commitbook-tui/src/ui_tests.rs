use std::path::PathBuf;

use commitbook_engine::git::ChangesSummary;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

use super::*;
use crate::app::LogEntry;

fn rendered_text(buffer: &Buffer, width: u16, height: u16) -> String {
    let mut text = String::new();
    for y in 0..height {
        for x in 0..width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

#[test]
fn draw_renders_panels_content_and_footer() {
    let mut app = App::blank(&PathBuf::from("/tmp/commitbook-tui-render-test"));
    app.running = true;
    app.schedule = "1h".into();
    app.schedule_desc = "Every hour".into();
    app.commit = "ai (claude)".into();
    app.conflicts = "both".into();
    app.log_keep = "30d".into();
    app.last_commit = Some("2026-08-09 12:00 UTC".into());
    app.log_lines = vec![LogEntry {
        timestamp: "12:00:00".into(),
        level: "INFO".into(),
        message: "Sync complete".into(),
    }];
    app.providers = vec![("codex-cli".into(), "Codex".into(), true)];
    app.changes = ChangesSummary {
        new_files: vec!["new.md".into()],
        modified_files: vec!["notes.md".into()],
        deleted_files: Vec::new(),
    };
    app.current_branch = "feature/security".into();

    let width = 120;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");

    terminal
        .draw(|frame| draw(frame, &app))
        .expect("TUI should render");

    let text = rendered_text(terminal.backend().buffer(), width, height);

    for title in ["Status", "Logs (1)", "Config", "Providers"] {
        assert!(text.contains(title), "missing panel title: {title}");
    }
    for content in [
        "Running",
        "feature/security",
        "2 pending (1 new, 1 modified)",
        "Sync complete",
        "schedule:      1h",
        "commit:        ai (claude)",
        "conflicts:     both",
        "keep logs:     30d",
        "Codex",
        "Tab:panel",
        "q:quit",
        "s:start/stop",
        "r:refresh",
        "↑↓:scroll status/logs",
        "p:preview",
    ] {
        assert!(
            text.contains(content),
            "missing rendered content: {content}"
        );
    }
}
