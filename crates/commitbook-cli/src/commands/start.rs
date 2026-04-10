use anyhow::{bail, Result};
use colored::Colorize;
use std::path::Path;

use commitbook_core::config::LocalConfig;
use commitbook_core::cron;
use commitbook_core::git::GitRepo;

pub fn run(_cb_dir: &Path, repo_root: &Path) -> Result<()> {
    if !GitRepo::is_repo(repo_root) {
        bail!("Not a git repository: {}", repo_root.display());
    }

    let config = LocalConfig::load(repo_root)?;

    // Get the binary path.
    let binary =
        std::env::current_exe().unwrap_or_else(|_| "commitbook".into());

    // Install the scheduler.
    let schedule = &config.schedule;
    cron::install(repo_root, schedule, &binary)?;

    println!("  {} Scheduler started.", "OK".green().bold());
    println!(
        "  Schedule: {}",
        cron::describe_schedule(schedule).cyan()
    );
    println!("  Command: {}", "commitbook run".dimmed());

    Ok(())
}
