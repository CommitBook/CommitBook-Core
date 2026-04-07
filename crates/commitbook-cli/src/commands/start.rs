use anyhow::{bail, Result};
use colored::*;
use std::path::Path;

use commitbook_core::config::{GlobalConfig, LocalConfig};
use commitbook_core::cron;
use commitbook_core::git::GitRepo;

pub fn run(repo_path: &Path) -> Result<()> {
    println!("{}", "CommitBook Start".bold().cyan());
    println!();

    if !LocalConfig::exists(repo_path) {
        bail!("CommitBook is not set up in this repository.\nRun {} first.", "commitbook setup".bold());
    }

    let mut config = LocalConfig::load(repo_path)?;

    // Check if already running (use is_loaded for ground truth, not just config)
    if cron::is_loaded(repo_path) {
        println!("  {} Auto-commits are already running.", "!".yellow().bold());
        println!("  Schedule: {}", cron::describe_schedule(&config.schedule).cyan());
        return Ok(());
    }

    if !GitRepo::is_repo(repo_path) {
        bail!("Directory is no longer a git repository: {}", repo_path.display());
    }

    let commitbook_bin = std::env::current_exe()?;

    print!("  Installing scheduler... ");
    let scheduler_id = cron::install(repo_path, &config.schedule, &commitbook_bin)?;
    println!("{}", "OK".green().bold());

    config.enabled = true;
    config.scheduler_id = Some(scheduler_id);
    config.save(repo_path)?;

    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut global = GlobalConfig::load()?;
    global.set_repo_enabled(&repo_str, true)?;

    println!();
    println!("  {} Auto-commits are now active!", "OK".green().bold());
    println!("  Schedule: {}", cron::describe_schedule(&config.schedule).cyan());
    println!();
    println!("  Run {} to stop.", "commitbook stop".bold());

    Ok(())
}
