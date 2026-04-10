use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_core::config::LocalConfig;
use commitbook_core::cron;
use commitbook_core::git::GitRepo;
use commitbook_core::state::sync_state::SyncState;

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
            "remote_head": state.remote_head,
            "last_sync_at": state.last_sync_at,
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

    if let Some(ref head) = state.remote_head {
        println!(
            "  Remote HEAD: {}",
            &head[..7.min(head.len())]
        );
    }

    Ok(())
}
