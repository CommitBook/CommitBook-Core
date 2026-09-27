use super::*;

#[test]
fn test_panel_next_cycles() {
    assert_eq!(Panel::Status.next(), Panel::Logs);
    assert_eq!(Panel::Logs.next(), Panel::Config);
    assert_eq!(Panel::Config.next(), Panel::Providers);
    assert_eq!(Panel::Providers.next(), Panel::Status);
}

#[test]
fn test_panel_prev_cycles() {
    assert_eq!(Panel::Status.prev(), Panel::Providers);
    assert_eq!(Panel::Logs.prev(), Panel::Status);
    assert_eq!(Panel::Config.prev(), Panel::Logs);
    assert_eq!(Panel::Providers.prev(), Panel::Config);
}

#[test]
fn test_log_entry_parse_valid() {
    let line = r#"{"ts":"2026-04-07 14:30:02","level":"INFO","msg":"Committed abc1234"}"#;
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.level, "INFO");
    assert_eq!(entry.message, "Committed abc1234");
    assert_eq!(entry.timestamp, "2026-04-07 14:30:02");
}

#[test]
fn test_log_entry_parse_invalid() {
    assert!(LogEntry::parse("not json").is_none());
}

#[test]
fn test_handle_key_quit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = App::blank(tmp.path());

    app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(app.quit);
}

#[test]
fn test_handle_key_tab() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = App::blank(tmp.path());

    app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.active_panel, Panel::Logs);

    app.handle_key(KeyCode::BackTab, KeyModifiers::NONE);
    assert_eq!(app.active_panel, Panel::Status);
}

#[test]
fn test_handle_key_scroll() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = App::blank(tmp.path());
    app.active_panel = Panel::Logs;
    app.log_lines = vec![
        LogEntry {
            timestamp: "t1".into(),
            level: "INFO".into(),
            message: "m1".into(),
        },
        LogEntry {
            timestamp: "t2".into(),
            level: "INFO".into(),
            message: "m2".into(),
        },
        LogEntry {
            timestamp: "t3".into(),
            level: "INFO".into(),
            message: "m3".into(),
        },
    ];

    app.handle_key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.log_scroll, 1);

    app.handle_key(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(app.log_scroll, 0);

    // Can't scroll above 0
    app.handle_key(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(app.log_scroll, 0);
}

#[test]
fn refresh_clears_provider_status_when_disabled_or_config_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    LocalConfig::init(tmp.path(), &LocalConfig::new("notes", "main", "origin")).unwrap();
    let mut app = App::new(tmp.path());
    assert!(app.providers.is_empty());
    for invalid_config in [false, true] {
        app.providers = vec![("codex-cli".into(), "Codex".into(), true)];
        if invalid_config {
            std::fs::write(LocalConfig::config_path(tmp.path()), "invalid = [").unwrap();
        }
        app.refresh();
        assert!(app.providers.is_empty());
    }
}

#[test]
fn background_refresh_keeps_navigation_and_action_feedback() {
    let tmp = tempfile::tempdir().unwrap();
    LocalConfig::init(tmp.path(), &LocalConfig::new("notes", "main", "origin")).unwrap();
    let mut app = App::blank(tmp.path());
    let refresh = start_refresh(tmp.path(), false);
    app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
    app.log_scroll = 4;
    app.status_scroll = 2;
    app.action_error = Some("scheduler failed".into());
    app.refreshing = true;

    app.apply_refresh(refresh.join().unwrap());

    assert_eq!(app.branch, "main");
    assert_eq!(app.active_panel, Panel::Logs);
    assert_eq!(app.log_scroll, 4);
    assert_eq!(app.status_scroll, 2);
    assert_eq!(app.action_error.as_deref(), Some("scheduler failed"));
    assert!(!app.refreshing);
}

#[test]
fn preview_navigation_refreshes_and_returns_to_dashboard() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    LocalConfig::init(tmp.path(), &LocalConfig::new("notes", "main", "origin")).unwrap();
    let mut app = App::new(tmp.path());
    app.handle_key(KeyCode::Char('p'), KeyModifiers::NONE);
    assert!(app.preview.is_some());
    app.handle_key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.preview_scroll, 1);
    std::fs::write(tmp.path().join("new.json"), "{}").unwrap();
    app.handle_key(KeyCode::Char('r'), KeyModifiers::NONE);
    assert!(app
        .preview
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .any(|e| e.path == "new.json"));
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.preview.is_none());
    assert!(!app.quit);
    app.active_panel = Panel::Status;
    app.handle_key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.status_scroll, 1);
    app.handle_key(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(app.status_scroll, 0);
    assert_eq!(app.commit, "timestamp");
    assert_eq!(app.conflicts, "both");
    assert_eq!(app.log_keep, "30d");
    std::fs::write(LocalConfig::config_path(tmp.path()), "broken = [").unwrap();
    app.refresh();
    assert!(app.schedule.is_empty());
    assert_eq!(app.branch, "unknown");
    assert_eq!(app.commit, "unknown");
    assert_eq!(app.conflicts, "unknown");
}
