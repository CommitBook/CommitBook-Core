use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_core::config::LocalConfig;

pub fn run(_cb_dir: &Path, repo_root: &Path, lines: usize, json: bool) -> Result<()> {
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

    let mut shown = 0;

    for entry in &log_files {
        if shown >= lines {
            break;
        }

        let content = std::fs::read_to_string(entry.path())?;
        for line in content.lines().rev() {
            if shown >= lines {
                break;
            }
            if line.trim().is_empty() {
                continue;
            }

            if json {
                println!("{}", line);
            } else {
                println!("  {}", format_log_line(line));
            }
            shown += 1;
        }
    }

    if shown == 0 {
        println!("{}", "No log entries found.".dimmed());
    }

    Ok(())
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
