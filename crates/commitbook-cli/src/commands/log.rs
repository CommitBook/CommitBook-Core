use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_core::config::LocalConfig;

pub fn run(_cb_dir: &Path, repo_root: &Path, lines: usize, _json: bool) -> Result<()> {
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

    log_files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

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

            // Colorize log levels.
            let colored = if line.contains("[ERROR]") {
                line.replace("[ERROR]", &"[ERROR]".red().to_string())
            } else if line.contains("[WARN]") {
                line.replace("[WARN]", &"[WARN]".yellow().to_string())
            } else if line.contains("[INFO]") {
                line.replace("[INFO]", &"[INFO]".green().to_string())
            } else {
                line.to_string()
            };

            println!("  {}", colored);
            shown += 1;
        }
    }

    if shown == 0 {
        println!("{}", "No log entries found.".dimmed());
    }

    Ok(())
}
