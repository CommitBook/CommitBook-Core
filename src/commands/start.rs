use anyhow::{bail, Result};
use colored::*;
use std::path::Path;

use crate::config::{GlobalConfig, LocalConfig};
use crate::cron;
use crate::git::GitRepo;

/// Run the start command to enable auto-commits.
pub fn run(repo_path: &Path) -> Result<()> {
    println!("{}", "CommitBook Start".bold().cyan());
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

    if config.enabled && config.scheduler_id.is_some() {
        println!(
            "  {} Auto-commits are already running.",
            "!".yellow().bold()
        );
        println!(
            "  Schedule: {}",
            cron::describe_schedule(&config.schedule).cyan()
        );
        return Ok(());
    }

    // 3. Verify it's still a valid git repo
    if !GitRepo::is_repo(repo_path) {
        bail!("Directory is no longer a git repository: {}", repo_path.display());
    }

    // 4. Get the commitbook binary path
    let commitbook_bin = std::env::current_exe()?;

    // 5. Install cron/launchd job
    print!("  Installing scheduler... ");
    let scheduler_id = cron::install(repo_path, &config.schedule, &commitbook_bin)?;
    println!("{}", "OK".green().bold());

    // 6. Update config
    config.enabled = true;
    config.scheduler_id = Some(scheduler_id);
    config.save(repo_path)?;

    // 7. Update global config
    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut global = GlobalConfig::load()?;
    global.set_repo_enabled(&repo_str, true)?;

    println!();
    println!(
        "  {} Auto-commits are now active!",
        "✓".green().bold()
    );
    println!(
        "  Schedule: {}",
        cron::describe_schedule(&config.schedule).cyan()
    );
    println!();
    println!(
        "  Run {} to stop auto-commits.",
        "commitbook stop".bold()
    );

    Ok(())
}
