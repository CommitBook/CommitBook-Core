use anyhow::{bail, Result};
use colored::*;
use std::io::{self, Write};
use std::path::Path;

use commitbook_core::config::local::LocalConfig;
use commitbook_core::config::global::GlobalConfig;
use commitbook_core::cron;

pub fn run(repo_path: &Path, force: bool) -> Result<()> {
    if !LocalConfig::exists(repo_path) {
        bail!("CommitBook is not configured in this repository.");
    }

    if !force {
        print!(
            "{} Remove all CommitBook data from {}? [y/N] ",
            "Warning:".yellow().bold(),
            repo_path.display()
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // 1. Stop scheduler
    if cron::is_loaded(repo_path) {
        cron::uninstall(repo_path, None)?;
        println!("  {} Scheduler stopped", "✓".green());
    }

    // 2. Remove .CommitBook directory
    let cb_dir = LocalConfig::commitbook_dir(repo_path);
    if cb_dir.exists() {
        std::fs::remove_dir_all(&cb_dir)?;
        println!("  {} Removed {}", "✓".green(), cb_dir.display());
    }

    // 3. Deregister from global config
    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    if let Ok(mut global) = GlobalConfig::load() {
        if global.repos.remove(&repo_str).is_some() {
            let _ = global.save();
            println!("  {} Deregistered from global config", "✓".green());
        }
    }

    println!();
    println!("{}", "CommitBook has been removed from this repository.".green());
    Ok(())
}
