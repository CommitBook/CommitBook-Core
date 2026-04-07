use anyhow::{bail, Result};
use colored::*;
use std::path::Path;

use commitbook_core::config::{GlobalConfig, LocalConfig};
use commitbook_core::cron;

pub fn run(repo_path: &Path, expression: &str) -> Result<()> {
    println!("{}", "CommitBook Set Schedule".bold().cyan());
    println!();

    if !LocalConfig::exists(repo_path) {
        bail!("CommitBook is not set up in this repository.\nRun {} first.", "commitbook setup".bold());
    }

    let schedule = cron::resolve_schedule(expression);
    cron::validate_cron_expression(&schedule)?;

    let mut config = LocalConfig::load(repo_path)?;
    let old_schedule = config.schedule.clone();

    if old_schedule == schedule {
        println!("  {} Schedule is already set to {}.", "!".yellow().bold(), cron::describe_schedule(&schedule).cyan());
        return Ok(());
    }

    config.schedule = schedule.clone();
    config.save(repo_path)?;

    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut global = GlobalConfig::load()?;
    global.set_repo_schedule(&repo_str, &schedule)?;

    // If running, reinstall the scheduler with new interval
    if config.enabled && cron::is_loaded(repo_path) {
        print!("  Updating scheduler... ");
        cron::uninstall(repo_path, config.scheduler_id.as_deref())?;
        let commitbook_bin = std::env::current_exe()?;
        let scheduler_id = cron::install(repo_path, &schedule, &commitbook_bin)?;
        config.scheduler_id = Some(scheduler_id);
        config.save(repo_path)?;
        println!("{}", "OK".green().bold());
    }

    println!();
    println!("  {} Schedule updated.", "OK".green().bold());
    println!("  Old: {}", cron::describe_schedule(&old_schedule).dimmed());
    println!("  New: {}", cron::describe_schedule(&schedule).cyan());

    Ok(())
}
