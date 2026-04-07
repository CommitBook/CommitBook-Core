use anyhow::{bail, Result};
use colored::*;
use std::path::Path;

use commitbook_core::config::LocalConfig;
use commitbook_core::logger::FileLogger;

pub fn run(repo_path: &Path, max_lines: usize) -> Result<()> {
    if !LocalConfig::exists(repo_path) {
        bail!("CommitBook is not set up in this repository.\nRun {} first.", "commitbook setup".bold());
    }

    let config = LocalConfig::load(repo_path)?;
    let logger = FileLogger::new(repo_path, config.logging.max_log_files)?;

    let lines = logger.read_recent(max_lines)?;

    if lines.is_empty() {
        println!("{}", "No log entries found.".dimmed());
        return Ok(());
    }

    println!("{}", "CommitBook Log".bold().cyan());
    println!();

    // Lines are in reverse order (most recent first)
    for line in lines.iter().rev() {
        // Try to parse as JSON for colorized output
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            let ts = entry.get("ts").and_then(|v| v.as_str()).unwrap_or("");
            let level = entry.get("level").and_then(|v| v.as_str()).unwrap_or("");
            let msg = entry.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            let provider = entry.get("provider").and_then(|v| v.as_str());

            let level_colored = match level {
                "ERROR" => level.red().bold().to_string(),
                "WARN" => level.yellow().bold().to_string(),
                "DEBUG" => level.dimmed().to_string(),
                _ => level.green().to_string(),
            };

            if let Some(p) = provider {
                println!("  [{}] [{}] {} ({})", ts.dimmed(), level_colored, msg, p.cyan());
            } else {
                println!("  [{}] [{}] {}", ts.dimmed(), level_colored, msg);
            }
        } else {
            // Fallback: print raw line
            println!("  {}", line);
        }
    }

    Ok(())
}
