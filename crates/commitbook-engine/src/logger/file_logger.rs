use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::LogKeep;
use crate::platform::{LogLevel, Logger};
use crate::utils::datetime;

/// File-based logger that writes JSON-lines to .CommitBook/local/logs/YYYY-MM-DD.log
///
/// Each line is a self-contained JSON object:
/// ```json
/// {"ts":"2026-04-07 14:30:02","level":"INFO","msg":"Committed abc1234","provider":"Claude Code"}
/// ```
pub struct FileLogger {
    logs_dir: PathBuf,
    /// Days of daily logs to keep; `None` keeps them forever.
    keep_days: Option<u32>,
}

impl FileLogger {
    /// Inspect existing logs without creating directories.
    pub fn read_only(repo_path: &Path) -> Self {
        Self {
            logs_dir: repo_path.join(".CommitBook/local/logs"),
            keep_days: None,
        }
    }
    pub fn new(repo_path: &Path, keep: LogKeep) -> Result<Self> {
        let logs_dir = repo_path.join(".CommitBook").join("local").join("logs");
        fs::create_dir_all(&logs_dir)
            .with_context(|| format!("Failed to create logs directory: {}", logs_dir.display()))?;
        Ok(Self {
            logs_dir,
            keep_days: keep.days(),
        })
    }

    /// Write a structured log entry with optional extra fields.
    pub fn log_with(
        &self,
        level: &str,
        message: &str,
        extra: Option<&[(&str, &str)]>,
    ) -> Result<()> {
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

        let mut line =
            serde_json::to_string(&entry).with_context(|| "Failed to serialize log entry")?;
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

    /// Remove log files older than the retention, using date-based filename
    /// parsing instead of mtime (more reliable, immune to `touch` and file
    /// sync tools). Keeps everything when retention is `forever`.
    pub fn cleanup_old_logs(&self) -> Result<()> {
        let Some(keep_days) = self.keep_days else {
            return Ok(());
        };

        let cutoff =
            chrono::Local::now().date_naive() - chrono::Duration::days(i64::from(keep_days));

        let entries =
            fs::read_dir(&self.logs_dir).with_context(|| "Failed to read logs directory")?;

        for entry in entries.flatten() {
            let path = entry.path();

            let is_log = path.extension().map(|ext| ext == "log").unwrap_or(false);
            if !is_log {
                continue;
            }

            // Parse date from filename: "2026-04-07.log" → NaiveDate
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

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

        let content = fs::read_to_string(&log_path).with_context(|| "Failed to read log file")?;

        let lines: Vec<String> = content
            .lines()
            .rev()
            .take(max_lines)
            .map(|s| s.to_string())
            .collect();

        Ok(lines)
    }

    /// Read log entries across all log files with pagination.
    ///
    /// Returns lines in reverse chronological order (newest first).
    /// `offset` skips the first N entries, `limit` caps the result size.
    pub fn read_entries(&self, limit: usize, offset: usize) -> Result<Vec<String>> {
        let mut log_files: Vec<PathBuf> = fs::read_dir(&self.logs_dir)
            .with_context(|| "Failed to read logs directory")?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|ext| ext == "log").unwrap_or(false))
            .filter(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .is_some()
            })
            .collect();

        // Sort by filename descending (newest date first)
        log_files.sort_by(|a, b| b.cmp(a));

        self.collect_entries(&log_files, limit, offset, None)
    }

    /// Read log entries filtered by level, with pagination.
    ///
    /// Returns lines in reverse chronological order (newest first).
    /// Only lines whose JSON `"level"` field matches `level` (case-sensitive) are included.
    /// `offset` and `limit` apply after filtering.
    pub fn read_entries_filtered(
        &self,
        limit: usize,
        offset: usize,
        level: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut log_files: Vec<PathBuf> = fs::read_dir(&self.logs_dir)
            .with_context(|| "Failed to read logs directory")?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|ext| ext == "log").unwrap_or(false))
            .filter(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .is_some()
            })
            .collect();

        log_files.sort_by(|a, b| b.cmp(a));
        self.collect_entries(&log_files, limit, offset, level)
    }

    fn collect_entries(
        &self,
        log_files: &[PathBuf],
        limit: usize,
        offset: usize,
        level_filter: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut skipped = 0usize;
        let mut collected = Vec::new();

        for file in log_files {
            if collected.len() >= limit {
                break;
            }

            let content = fs::read_to_string(file)
                .with_context(|| format!("Failed to read log file: {}", file.display()))?;
            let mut lines: Vec<String> = content
                .lines()
                .filter(|l| !l.is_empty())
                .filter(|l| {
                    if let Some(filter) = level_filter {
                        serde_json::from_str::<serde_json::Value>(l)
                            .ok()
                            .and_then(|v| v["level"].as_str().map(|s| s == filter))
                            .unwrap_or(false)
                    } else {
                        true
                    }
                })
                .map(|s| s.to_string())
                .collect();
            // Reverse so newest entries within a file come first
            lines.reverse();

            for line in lines {
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                collected.push(line);
                if collected.len() >= limit {
                    break;
                }
            }
        }

        Ok(collected)
    }

    /// Returns the path to the logs directory.
    pub fn logs_dir(&self) -> &Path {
        &self.logs_dir
    }
}

impl Logger for FileLogger {
    fn emit(&self, level: LogLevel, message: &str) -> Result<()> {
        self.log(level.as_str(), message)
    }
}

#[cfg(test)]
#[path = "file_logger_tests.rs"]
mod tests;
