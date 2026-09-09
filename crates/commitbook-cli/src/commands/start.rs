use anyhow::{bail, Result};
use colored::Colorize;
use std::path::Path;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::cron::{self, SystemScheduler};
use commitbook_engine::git::GitRepo;
use commitbook_engine::settings::{self, SchedulerContext};

pub fn run(_cb_dir: &Path, repo_root: &Path) -> Result<()> {
    if !GitRepo::is_repo(repo_root) {
        bail!("Not a git repository: {}", repo_root.display());
    }

    let context = SchedulerContext::new(&SystemScheduler, settings::current_binary());
    settings::start_scheduler(repo_root, &context)?;
    let config = LocalConfig::load_read_only(repo_root)?;

    println!("  {} Scheduler started.", "OK".green().bold());
    println!(
        "  Schedule: {}",
        cron::describe_schedule(&config.schedule).cyan()
    );
    println!("  Command: {}", "commitbook run".dimmed());

    Ok(())
}
