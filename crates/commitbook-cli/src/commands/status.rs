use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::cron;
use commitbook_engine::git::GitRepo;
use commitbook_engine::logger::FileLogger;
use commitbook_engine::state::sync_state::SyncState;

pub fn run(cb_dir: &Path, repo_root: &Path, json: bool) -> Result<()> {
    let config = LocalConfig::load(repo_root)?;
    let state = SyncState::load(cb_dir)?;
    let repo = GitRepo::open(repo_root)?;

    if json {
        let obj = serde_json::json!({
            "enabled": config.enabled,
            "schedule": config.schedule,
            "branch": config.git.branch,
            "auto_push": config.git.auto_push,
            "last_sync_at": state.last_sync_at,
            "last_error": state.last_error,
            "has_remote": repo.has_remote(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    println!("{}", "CommitBook Status".bold().cyan());
    println!();

    // Scheduler state.
    let scheduler_running = cron::is_loaded(repo_root);
    if scheduler_running {
        println!("  Scheduler: {}", "running".green().bold());
    } else {
        println!("  Scheduler: {}", "stopped".yellow().bold());
    }

    println!(
        "  Schedule:  {}",
        cron::describe_schedule(&config.schedule).cyan()
    );
    println!("  Branch:    {}", config.git.branch);
    println!(
        "  Auto-push: {}",
        if config.git.auto_push { "yes" } else { "no" }
    );

    // Remote.
    if repo.has_remote() {
        println!("  Remote:    {}", "configured".green());
    } else {
        println!("  Remote:    {}", "none (local only)".yellow());
    }

    // Sync state.
    println!();
    if let Some(ref last) = state.last_sync_at {
        println!("  Last sync: {}", last);
    } else {
        println!("  Last sync: {}", "never".dimmed());
    }

    if let Some(ref err) = state.last_error {
        println!("  Last error: {}", err.red());
    }

    // Local vs remote divergence check: only meaningful when a remote is configured.
    if repo.has_remote() {
        let remote_ref = format!("{}/{}", config.git.remote, config.git.branch);
        if let Ok((ahead, behind)) = repo.ahead_behind("HEAD", &remote_ref) {
            if ahead > 0 || behind > 0 {
                println!(
                    "  Local vs {}: {} ahead, {} behind",
                    remote_ref, ahead, behind
                );
                if ahead > 0 && behind > 0 {
                    println!(
                        "  {} Local has diverged from {}. Run `commitbook doctor` for recovery.",
                        "WARN".yellow().bold(),
                        remote_ref
                    );
                }
            }
        }
    }

    // Recent activity, pull the last few JSON lines from today's log file
    // and render them human-readable. Skipped silently when there's no log
    // yet (FileLogger lazily creates the dir on first write).
    if let Ok(logger) = FileLogger::new(repo_root, config.logging.max_log_days) {
        if let Ok(lines) = logger.read_recent(5) {
            if !lines.is_empty() {
                println!();
                println!("  {}", "Recent activity:".bold());
                // read_recent returns newest-first; reverse so the most recent
                // entry sits at the bottom (matches `tail` conventions).
                for line in lines.iter().rev() {
                    if let Some(formatted) = format_recent(line) {
                        println!("    {}", formatted);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Render a JSON log line as `HH:MM:SS LEVEL msg`, or `None` if the line
/// can't be parsed.
fn format_recent(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let ts = v["ts"].as_str().unwrap_or("");
    let level = v["level"].as_str().unwrap_or("INFO");
    let msg = v["msg"].as_str().unwrap_or("");

    // ts comes in as "YYYY-MM-DD HH:MM:SS", keep just the time portion.
    let time = ts.split(' ').nth(1).unwrap_or(ts);

    let level_colored = match level {
        "ERROR" => level.red().bold().to_string(),
        "WARN" => level.yellow().bold().to_string(),
        "INFO" => level.green().bold().to_string(),
        other => other.to_string(),
    };

    Some(format!("{}  {:5}  {}", time.dimmed(), level_colored, msg))
}
