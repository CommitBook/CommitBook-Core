use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_engine::cron;

pub fn run(_cb_dir: &Path, repo_root: &Path) -> Result<()> {
    cron::uninstall(repo_root, None)?;

    println!("  {} Scheduler stopped.", "OK".green().bold());

    Ok(())
}
