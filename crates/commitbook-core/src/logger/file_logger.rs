use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::utils::datetime;

/// File-based logger that writes JSON-lines to .CommitBook/logs/YYYY-MM-DD.log
///
/// Each line is a self-contained JSON object:
/// ```json
/// {"ts":"2026-04-07 14:30:02","level":"INFO","msg":"Committed abc1234","provider":"Claude Code"}
/// ```
pub struct FileLogger {
    logs_dir: PathBuf,
    max_log_days: u32,
}

impl FileLogger {
    pub fn new(repo_path: &Path, max_log_days: u32) -> Result<Self> {
        let logs_dir = repo_path.join(".CommitBook").join("logs");
        fs::create_dir_all(&logs_dir)
            .with_context(|| format!("Failed to create logs directory: {}", logs_dir.display()))?;
        Ok(Self {
            logs_dir,
            max_log_days,
        })
    }

    /// Write a structured log entry with optional extra fields.
    pub fn log_with(&self, level: &str, message: &str, extra: Option<&[(&str, &str)]>) -> Result<()> {
        let filename = format!("{}.log", datetime::today_date());
        let log_path = self.logs_dir.join(&filename);
        let timestamp = datetime::now_formatted();

        let mut entry = json!({
            "ts": timestamp,
            "level": level,
            "msg": message,
        });

        if let Some(fields) = extra {
            if let Some(obj) = entry.as_object_mut() {
                for (k, v) in fields {
                    obj.insert(k.to_string(), serde_json::Value::String(v.to_string()));
                }
            }
        }

        let mut line = serde_json::to_string(&entry)
            .with_context(|| "Failed to serialize log entry")?;
        line.push('\n');

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

        file.write_all(line.as_bytes())
            .with_context(|| "Failed to write log entry")?;

        Ok(())
    }

    /// Write a simple log entry.
    pub fn log(&self, level: &str, message: &str) -> Result<()> {
        self.log_with(level, message, None)
    }

    pub fn info(&self, message: &str) -> Result<()> {
        self.log("INFO", message)
    }

    pub fn warn(&self, message: &str) -> Result<()> {
        self.log("WARN", message)
    }

    pub fn error(&self, message: &str) -> Result<()> {
        self.log("ERROR", message)
    }

    pub fn debug(&self, message: &str) -> Result<()> {
        self.log("DEBUG", message)
    }

    /// Remove log files older than max_log_days, using date-based filename parsing
    /// instead of mtime (more reliable — immune to `touch` and file sync tools).
    pub fn cleanup_old_logs(&self) -> Result<()> {
        if self.max_log_days == 0 {
            return Ok(());
        }

        let cutoff = chrono::Local::now()
            .date_naive()
            - chrono::Duration::days(self.max_log_days as i64);

        let entries = fs::read_dir(&self.logs_dir)
            .with_context(|| "Failed to read logs directory")?;

        for entry in entries.flatten() {
            let path = entry.path();

            let is_log = path
                .extension()
                .map(|ext| ext == "log")
                .unwrap_or(false);
            if !is_log {
                continue;
            }

            // Parse date from filename: "2026-04-07.log" → NaiveDate
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if let Ok(log_date) = chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d") {
                if log_date < cutoff {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        Ok(())
    }

    /// Read the most recent log entries from the current day's log file.
    pub fn read_recent(&self, max_lines: usize) -> Result<Vec<String>> {
        let filename = format!("{}.log", datetime::today_date());
        let log_path = self.logs_dir.join(&filename);

        if !log_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&log_path)
            .with_context(|| "Failed to read log file")?;

        let lines: Vec<String> = content
            .lines()
            .rev()
            .take(max_lines)
            .map(|s| s.to_string())
            .collect();

        Ok(lines)
    }

    /// Returns the path to the logs directory.
    pub fn logs_dir(&self) -> &Path {
        &self.logs_dir
    }
}

#[cfg(test)]
mod tests {
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
        assert!(tmp.path().join(".CommitBook").join("logs").exists());
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

        let levels: Vec<String> = lines.iter().map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["level"].as_str().unwrap().to_string()
        }).collect();
        // read_recent returns reversed (most recent first)
        assert!(levels.contains(&"INFO".to_string()));
        assert!(levels.contains(&"WARN".to_string()));
        assert!(levels.contains(&"ERROR".to_string()));
        assert!(levels.contains(&"DEBUG".to_string()));
    }

    #[test]
    fn test_extra_fields() {
        let (_tmp, logger) = make_logger();
        logger.log_with("INFO", "commit done", Some(&[("provider", "Claude"), ("hash", "abc1234")])).unwrap();

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
}
