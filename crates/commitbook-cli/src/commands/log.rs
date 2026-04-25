use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::time::Duration;

use commitbook_core::config::LocalConfig;

pub fn run(
    _cb_dir: &Path,
    repo_root: &Path,
    lines: usize,
    json: bool,
    tail: bool,
) -> Result<()> {
    let logs_dir = LocalConfig::logs_dir(repo_root);

    if !logs_dir.exists() {
        println!("{}", "No log entries found.".dimmed());
        return Ok(());
    }

    // Collect log files, sorted newest first.
    let mut log_files: Vec<_> = std::fs::read_dir(&logs_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "log")
        })
        .collect();

    log_files.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    if log_files.is_empty() {
        println!("{}", "No log entries found.".dimmed());
        return Ok(());
    }

    // Print the last N entries oldest-first so the most recent appears at the
    // bottom of the terminal (matches `tail` conventions).
    let mut collected: Vec<String> = Vec::new();
    for entry in &log_files {
        if collected.len() >= lines {
            break;
        }
        let content = std::fs::read_to_string(entry.path())?;
        for line in content.lines().rev() {
            if collected.len() >= lines {
                break;
            }
            if line.trim().is_empty() {
                continue;
            }
            collected.push(line.to_string());
        }
    }

    if collected.is_empty() {
        println!("{}", "No log entries found.".dimmed());
    } else {
        for line in collected.iter().rev() {
            print_line(line, json);
        }
    }

    if tail {
        follow_today(repo_root, json)?;
    }

    Ok(())
}

fn print_line(line: &str, json: bool) {
    if json {
        println!("{}", line);
    } else {
        println!("  {}", format_log_line(line));
    }
}

/// Poll the day's log file (and roll to tomorrow's at midnight) and print
/// every appended line. Runs until the user kills it with Ctrl-C.
fn follow_today(repo_root: &Path, json: bool) -> Result<()> {
    let logs_dir = LocalConfig::logs_dir(repo_root);
    let mut current_path = logs_dir.join(format!(
        "{}.log",
        chrono::Utc::now().format("%Y-%m-%d")
    ));
    let mut pos = std::fs::metadata(&current_path)
        .map(|m| m.len())
        .unwrap_or(0);

    loop {
        let today_path = logs_dir.join(format!(
            "{}.log",
            chrono::Utc::now().format("%Y-%m-%d")
        ));
        if today_path != current_path {
            // Day rolled over — start reading the new file from the start.
            current_path = today_path;
            pos = 0;
        }

        let len = std::fs::metadata(&current_path)
            .map(|m| m.len())
            .unwrap_or(0);
        if len > pos {
            use std::io::{Read, Seek, SeekFrom};
            if let Ok(mut f) = std::fs::File::open(&current_path) {
                if f.seek(SeekFrom::Start(pos)).is_ok() {
                    let mut buf = String::new();
                    if f.read_to_string(&mut buf).is_ok() {
                        for line in buf.lines() {
                            if !line.trim().is_empty() {
                                print_line(line, json);
                            }
                        }
                    }
                }
            }
            pos = len;
        } else if len < pos {
            // File rotated/truncated — restart.
            pos = 0;
        }

        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Format a JSON log line for human-readable output with colorized level.
fn format_log_line(line: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        let ts = v["ts"].as_str().unwrap_or("");
        let level = v["level"].as_str().unwrap_or("INFO");
        let msg = v["msg"].as_str().unwrap_or(line);

        let level_colored = match level {
            "ERROR" => format!("[{}]", level).red().to_string(),
            "WARN"  => format!("[{}]", level).yellow().to_string(),
            "INFO"  => format!("[{}]", level).green().to_string(),
            other   => format!("[{}]", other),
        };

        if ts.is_empty() {
            format!("{} {}", level_colored, msg)
        } else {
            format!("{} {} {}", ts, level_colored, msg)
        }
    } else {
        // Not JSON — print as-is.
        line.to_string()
    }
}

#[cfg(test)]
#[path = "log_tests.rs"]
mod tests;
