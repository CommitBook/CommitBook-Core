use anyhow::{bail, Result};
use colored::*;
use std::path::Path;

use commitbook_core::config::{GlobalConfig, LocalConfig};
use commitbook_core::cron;

pub fn run(repo_path: &Path) -> Result<()> {
    println!("{}", "CommitBook Stop".bold().cyan());
    println!();

    if !LocalConfig::exists(repo_path) {
        bail!("CommitBook is not set up in this repository.\nRun {} first.", "commitbook setup".bold());
    }

    let mut config = LocalConfig::load(repo_path)?;

    if !config.enabled && !cron::is_loaded(repo_path) {
        println!("  {} Auto-commits are already stopped.", "!".yellow().bold());
        return Ok(());
    }

    print!("  Removing scheduler... ");
    cron::uninstall(repo_path, config.scheduler_id.as_deref())?;
    println!("{}", "OK".green().bold());

    config.enabled = false;
    config.scheduler_id = None;
    config.save(repo_path)?;

    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut global = GlobalConfig::load()?;
    global.set_repo_enabled(&repo_str, false)?;

    println!();
    println!("  {} Auto-commits have been stopped.", "OK".green().bold());
    println!("  Your configuration and logs have been preserved.");
    println!();
    println!("  Run {} to restart.", "commitbook start".bold());

    Ok(())
}
