use anyhow::{bail, Result};
use colored::*;
use std::path::Path;

use crate::config::{GlobalConfig, LocalConfig};
use crate::cron;

/// Run the stop command to disable auto-commits.
pub fn run(repo_path: &Path) -> Result<()> {
    println!("{}", "CommitBook Stop".bold().cyan());
    println!();

    // 1. Validate repo is set up
    if !LocalConfig::exists(repo_path) {
        bail!(
            "CommitBook is not set up in this repository.\nRun {} first.",
            "commitbook setup".bold()
        );
    }

    // 2. Load config
    let mut config = LocalConfig::load(repo_path)?;

    if !config.enabled && config.scheduler_id.is_none() {
        println!(
            "  {} Auto-commits are already stopped.",
            "!".yellow().bold()
        );
        return Ok(());
    }

    // 3. Remove cron/launchd job
    print!("  Removing scheduler... ");
    cron::uninstall(repo_path, config.scheduler_id.as_deref())?;
    println!("{}", "OK".green().bold());

    // 4. Update local config
    config.enabled = false;
    config.scheduler_id = None;
    config.save(repo_path)?;

    // 5. Update global config
    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut global = GlobalConfig::load()?;
    global.set_repo_enabled(&repo_str, false)?;

    println!();
    println!(
        "  {} Auto-commits have been stopped.",
        "✓".green().bold()
    );
    println!("  Your configuration and logs have been preserved.");
    println!();
    println!(
        "  Run {} to restart auto-commits.",
        "commitbook start".bold()
    );

    Ok(())
}
