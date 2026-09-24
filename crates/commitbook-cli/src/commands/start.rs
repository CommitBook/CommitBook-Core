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

    let binary = settings::current_binary();
    let transient = cron::is_transient_binary(&binary);
    let context = SchedulerContext::new(&SystemScheduler, binary.clone());
    settings::start_scheduler(repo_root, &context)?;
    let config = LocalConfig::load_read_only(repo_root)?;

    println!("  {} Scheduler started.", "OK".green().bold());
    println!(
        "  Schedule: {}",
        cron::describe_schedule(&config.sync.schedule).cyan()
    );
    println!("  Command: {}", "commitbook sync".dimmed());
    if transient {
        println!(
            "  {} {} is a build artifact; the scheduler stops when it is removed. Install `commitbook` and run `commitbook start` from it.",
            "Warning:".yellow().bold(),
            binary.display()
        );
    }

    Ok(())
}
