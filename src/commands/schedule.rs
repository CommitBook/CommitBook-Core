use anyhow::{bail, Result};
use colored::*;
use std::path::Path;

use crate::config::{GlobalConfig, LocalConfig};
use crate::cron;

/// Run the set-schedule command to change the commit schedule.
pub fn run(repo_path: &Path, expression: &str) -> Result<()> {
    println!("{}", "CommitBook Set Schedule".bold().cyan());
    println!();

    // 1. Validate repo is set up
    if !LocalConfig::exists(repo_path) {
        bail!(
            "CommitBook is not set up in this repository.\nRun {} first.",
            "commitbook setup".bold()
        );
    }

    // 2. Resolve and validate the schedule
    let schedule = cron::resolve_schedule(expression);
    cron::validate_cron_expression(&schedule)?;

    // 3. Load current config
    let mut config = LocalConfig::load(repo_path)?;
    let old_schedule = config.schedule.clone();

    if old_schedule == schedule {
        println!(
            "  {} Schedule is already set to {}.",
            "!".yellow().bold(),
            cron::describe_schedule(&schedule).cyan()
        );
        return Ok(());
    }

    // 4. Update local config
    config.schedule = schedule.clone();
    config.save(repo_path)?;

    // 5. Update global config
    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut global = GlobalConfig::load()?;
    global.set_repo_schedule(&repo_str, &schedule)?;

    // 6. If auto-commits are running, reinstall the scheduler
    if config.enabled && config.scheduler_id.is_some() {
        print!("  Updating scheduler... ");
        // Remove old
        cron::uninstall(repo_path, config.scheduler_id.as_deref())?;
        // Install new
        let commitbook_bin = std::env::current_exe()?;
        let scheduler_id = cron::install(repo_path, &schedule, &commitbook_bin)?;
        config.scheduler_id = Some(scheduler_id);
        config.save(repo_path)?;
        println!("{}", "OK".green().bold());
    }

    println!();
    println!(
        "  {} Schedule updated.",
        "✓".green().bold()
    );
    println!(
        "  Old: {}",
        cron::describe_schedule(&old_schedule).dimmed()
    );
    println!(
        "  New: {}",
        cron::describe_schedule(&schedule).cyan()
    );

    Ok(())
}
