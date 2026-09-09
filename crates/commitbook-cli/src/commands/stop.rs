use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_engine::cron::SystemScheduler;
use commitbook_engine::settings::{self, SchedulerContext};

pub fn run(_cb_dir: &Path, repo_root: &Path) -> Result<()> {
    let context = SchedulerContext::new(&SystemScheduler, settings::current_binary());
    settings::stop_scheduler(repo_root, &context)?;

    println!("  {} Scheduler stopped.", "OK".green().bold());

    Ok(())
}
