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
