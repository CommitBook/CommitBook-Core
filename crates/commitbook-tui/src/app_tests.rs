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
    let mut app = App {
        repo_path: tmp.path().to_path_buf(),
        active_panel: Panel::Status,
        running: false,
        schedule: String::new(),
        schedule_desc: String::new(),
        auto_push: true,
        branch: "main".into(),
        last_commit: None,
        log_lines: Vec::new(),
        log_scroll: 0,
        providers: Vec::new(),
        changes: ChangesSummary::default(),
        current_branch: "main".into(),
        enabled: true,
        log_level: "info".into(),
        quit: false,
    };

    app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(app.quit);
}

#[test]
fn test_handle_key_tab() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = App {
        repo_path: tmp.path().to_path_buf(),
        active_panel: Panel::Status,
        running: false,
        schedule: String::new(),
        schedule_desc: String::new(),
        auto_push: true,
        branch: "main".into(),
        last_commit: None,
        log_lines: Vec::new(),
        log_scroll: 0,
        providers: Vec::new(),
        changes: ChangesSummary::default(),
        current_branch: "main".into(),
        enabled: true,
        log_level: "info".into(),
        quit: false,
    };

    app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.active_panel, Panel::Logs);

    app.handle_key(KeyCode::BackTab, KeyModifiers::NONE);
    assert_eq!(app.active_panel, Panel::Status);
}

#[test]
fn test_handle_key_scroll() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = App {
        repo_path: tmp.path().to_path_buf(),
        active_panel: Panel::Logs,
        running: false,
        schedule: String::new(),
        schedule_desc: String::new(),
        auto_push: true,
        branch: "main".into(),
        last_commit: None,
        log_lines: vec![
            LogEntry { timestamp: "t1".into(), level: "INFO".into(), message: "m1".into() },
            LogEntry { timestamp: "t2".into(), level: "INFO".into(), message: "m2".into() },
            LogEntry { timestamp: "t3".into(), level: "INFO".into(), message: "m3".into() },
        ],
        log_scroll: 0,
        providers: Vec::new(),
        changes: ChangesSummary::default(),
        current_branch: "main".into(),
        enabled: true,
        log_level: "info".into(),
        quit: false,
    };

    app.handle_key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.log_scroll, 1);

    app.handle_key(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(app.log_scroll, 0);

    // Can't scroll above 0
    app.handle_key(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(app.log_scroll, 0);
}
