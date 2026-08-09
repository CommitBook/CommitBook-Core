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
    let app = App {
        repo_path: PathBuf::from("/tmp/commitbook-tui-render-test"),
        active_panel: Panel::Status,
        running: true,
        schedule: "0 * * * *".into(),
        schedule_desc: "hourly".into(),
        auto_push: true,
        branch: "main".into(),
        last_commit: Some("2026-08-09 12:00 UTC".into()),
        log_lines: vec![LogEntry {
            timestamp: "12:00:00".into(),
            level: "INFO".into(),
            message: "Sync complete".into(),
        }],
        log_scroll: 0,
        providers: vec![("codex-cli".into(), "Codex".into(), true)],
        changes: ChangesSummary {
            new_files: vec!["new.md".into()],
            modified_files: vec!["notes.md".into()],
            deleted_files: Vec::new(),
        },
        current_branch: "feature/security".into(),
        enabled: true,
        log_level: "info".into(),
        quit: false,
    };

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
        "schedule:      0 * * * *",
        "Codex",
        "Tab:panel",
        "q:quit",
        "s:start/stop",
        "r:refresh",
        "↑↓:scroll logs",
    ] {
        assert!(
            text.contains(content),
            "missing rendered content: {content}"
        );
    }
}
