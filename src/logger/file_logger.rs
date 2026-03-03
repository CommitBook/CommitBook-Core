use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::utils::datetime;

/// File-based logger that writes to .CommitBook/logs/YYYY-MM-DD.log
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

    /// Write a log entry at the given level.
    pub fn log(&self, level: &str, message: &str) -> Result<()> {
        let filename = format!("{}.log", datetime::today_date());
        let log_path = self.logs_dir.join(&filename);
        let timestamp = datetime::now_formatted();
        let entry = format!("[{}] [{}] {}\n", timestamp, level, message);

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

        file.write_all(entry.as_bytes())
            .with_context(|| "Failed to write log entry")?;

        Ok(())
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

    /// Remove log files older than max_log_days.
    pub fn cleanup_old_logs(&self) -> Result<()> {
        let entries = fs::read_dir(&self.logs_dir)
            .with_context(|| "Failed to read logs directory")?;

        let cutoff = chrono::Local::now()
            - chrono::Duration::days(self.max_log_days as i64);

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext != "log" {
                    continue;
                }
            } else {
                continue;
            }

            if let Ok(metadata) = fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    let modified_time: chrono::DateTime<chrono::Local> = modified.into();
                    if modified_time < cutoff {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }

        Ok(())
    }
}
