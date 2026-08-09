use super::*;

fn make_logger() -> (tempfile::TempDir, FileLogger) {
    let tmp = tempfile::tempdir().unwrap();
    let logger = FileLogger::new(tmp.path(), 10).unwrap();
    (tmp, logger)
}

#[test]
fn test_creates_logs_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let _logger = FileLogger::new(tmp.path(), 10).unwrap();
    assert!(tmp
        .path()
        .join(".CommitBook")
        .join("local")
        .join("logs")
        .exists());
}

#[test]
fn test_writes_valid_json() {
    let (_tmp, logger) = make_logger();
    logger.info("test message").unwrap();

    let lines = logger.read_recent(10).unwrap();
    assert_eq!(lines.len(), 1);

    let entry: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(entry["level"], "INFO");
    assert_eq!(entry["msg"], "test message");
    assert!(entry["ts"].is_string());
}

#[test]
fn test_all_log_levels() {
    let (_tmp, logger) = make_logger();
    logger.info("i").unwrap();
    logger.warn("w").unwrap();
    logger.error("e").unwrap();
    logger.debug("d").unwrap();

    let lines = logger.read_recent(10).unwrap();
    assert_eq!(lines.len(), 4);

    let levels: Vec<String> = lines
        .iter()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["level"].as_str().unwrap().to_string()
        })
        .collect();
    // read_recent returns reversed (most recent first)
    assert!(levels.contains(&"INFO".to_string()));
    assert!(levels.contains(&"WARN".to_string()));
    assert!(levels.contains(&"ERROR".to_string()));
    assert!(levels.contains(&"DEBUG".to_string()));
}

#[test]
fn test_extra_fields() {
    let (_tmp, logger) = make_logger();
    logger
        .log_with(
            "INFO",
            "commit done",
            Some(&[("provider", "Claude"), ("hash", "abc1234")]),
        )
        .unwrap();

    let lines = logger.read_recent(10).unwrap();
    let entry: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(entry["provider"], "Claude");
    assert_eq!(entry["hash"], "abc1234");
}

#[test]
fn test_read_recent_empty() {
    let (_tmp, logger) = make_logger();
    let lines = logger.read_recent(10).unwrap();
    assert!(lines.is_empty());
}

#[test]
fn test_read_recent_limits() {
    let (_tmp, logger) = make_logger();
    for i in 0..5 {
        logger.info(&format!("msg {}", i)).unwrap();
    }
    let lines = logger.read_recent(3).unwrap();
    assert_eq!(lines.len(), 3);
}

#[test]
fn test_cleanup_removes_old_files() {
    let (_tmp, logger) = make_logger();
    // Create an old log file (10+ days old)
    let old_date = (chrono::Local::now() - chrono::Duration::days(15))
        .format("%Y-%m-%d")
        .to_string();
    let old_path = logger.logs_dir().join(format!("{}.log", old_date));
    fs::write(&old_path, "old entry\n").unwrap();

    logger.cleanup_old_logs().unwrap();
    assert!(!old_path.exists());
}

#[test]
fn test_cleanup_keeps_recent_files() {
    let (_tmp, logger) = make_logger();
    // Create a recent log file (2 days old)
    let recent_date = (chrono::Local::now() - chrono::Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();
    let recent_path = logger.logs_dir().join(format!("{}.log", recent_date));
    fs::write(&recent_path, "recent entry\n").unwrap();

    logger.cleanup_old_logs().unwrap();
    assert!(recent_path.exists());
}

#[test]
fn test_cleanup_ignores_non_log_files() {
    let (_tmp, logger) = make_logger();
    let txt_path = logger.logs_dir().join("notes.txt");
    fs::write(&txt_path, "not a log\n").unwrap();

    logger.cleanup_old_logs().unwrap();
    assert!(txt_path.exists());
}

#[test]
fn test_read_entries_multi_day() {
    let (_tmp, logger) = make_logger();

    // Write entries to two different date files
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    let today_path = logger.logs_dir().join(format!("{}.log", today));
    let yesterday_path = logger.logs_dir().join(format!("{}.log", yesterday));

    fs::write(&yesterday_path, "{\"ts\":\"y1\",\"level\":\"INFO\",\"msg\":\"old1\"}\n{\"ts\":\"y2\",\"level\":\"INFO\",\"msg\":\"old2\"}\n").unwrap();
    fs::write(&today_path, "{\"ts\":\"t1\",\"level\":\"INFO\",\"msg\":\"new1\"}\n{\"ts\":\"t2\",\"level\":\"INFO\",\"msg\":\"new2\"}\n").unwrap();

    let entries = logger.read_entries(10, 0).unwrap();
    // Should get 4 entries, today's newest first, then yesterday's newest
    assert_eq!(entries.len(), 4);
    assert!(entries[0].contains("new2")); // today's last line (newest)
    assert!(entries[1].contains("new1")); // today's first line
    assert!(entries[2].contains("old2")); // yesterday's last line
    assert!(entries[3].contains("old1")); // yesterday's first line
}

#[test]
fn test_read_entries_with_offset() {
    let (_tmp, logger) = make_logger();
    for i in 0..5 {
        logger.info(&format!("msg {}", i)).unwrap();
    }

    let entries = logger.read_entries(10, 2).unwrap();
    assert_eq!(entries.len(), 3); // skipped 2 of 5
}

#[test]
fn test_read_entries_with_limit() {
    let (_tmp, logger) = make_logger();
    for i in 0..5 {
        logger.info(&format!("msg {}", i)).unwrap();
    }

    let entries = logger.read_entries(2, 0).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_read_entries_empty() {
    let (_tmp, logger) = make_logger();
    let entries = logger.read_entries(10, 0).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn test_read_entries_offset_beyond_end() {
    let (_tmp, logger) = make_logger();
    logger.info("only one").unwrap();

    let entries = logger.read_entries(10, 100).unwrap();
    assert!(entries.is_empty());
}
