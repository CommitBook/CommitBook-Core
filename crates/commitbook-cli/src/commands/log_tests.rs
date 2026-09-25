use super::format_log_line;

#[test]
fn test_format_log_line_info() {
    let line = r#"{"ts":"2026-04-07 14:30:02","level":"INFO","msg":"Committed abc1234"}"#;
    let formatted = format_log_line(line);
    assert!(formatted.contains("2026-04-07 14:30:02"));
    assert!(formatted.contains("INFO"));
    assert!(formatted.contains("Committed abc1234"));
}

#[test]
fn test_format_log_line_error() {
    let line = r#"{"ts":"2026-04-07 15:00:00","level":"ERROR","msg":"Sync failed"}"#;
    let formatted = format_log_line(line);
    assert!(formatted.contains("ERROR"));
    assert!(formatted.contains("Sync failed"));
}

#[test]
fn test_format_log_line_warn() {
    let line = r#"{"ts":"2026-04-07 16:00:00","level":"WARN","msg":"No remote configured"}"#;
    let formatted = format_log_line(line);
    assert!(formatted.contains("WARN"));
    assert!(formatted.contains("No remote configured"));
}

#[test]
fn test_format_log_line_non_json_passthrough() {
    let line = "some plain text line";
    let formatted = format_log_line(line);
    assert_eq!(formatted, "some plain text line");
}

#[test]
fn test_format_log_line_no_ts() {
    let line = r#"{"level":"INFO","msg":"No timestamp"}"#;
    let formatted = format_log_line(line);
    assert!(formatted.contains("INFO"));
    assert!(formatted.contains("No timestamp"));
}

#[test]
fn recent_lines_skip_launchd_captures_and_keep_oldest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp.path().join(".CommitBook/local/logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(
        logs.join("2026-09-24.log"),
        "{\"msg\":\"day one a\"}\n{\"msg\":\"day one b\"}\n",
    )
    .unwrap();
    std::fs::write(logs.join("2026-09-25.log"), "{\"msg\":\"day two\"}\n").unwrap();
    // Sorts after the dated files by name; used to crowd them out.
    std::fs::write(
        logs.join("launchd-stdout.log"),
        "  OK Pushed 1 commit(s).\n",
    )
    .unwrap();
    std::fs::write(logs.join("launchd-stderr.log"), "ERROR boom\n").unwrap();

    let lines = super::recent_lines(tmp.path(), 2).unwrap();
    assert_eq!(lines, ["{\"msg\":\"day one b\"}", "{\"msg\":\"day two\"}"]);
}
