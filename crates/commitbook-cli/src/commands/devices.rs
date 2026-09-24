use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_engine::devices;
use commitbook_engine::state::RepoLock;

/// List every device syncing this CommitBook.
pub fn list(repo_root: &Path, json: bool) -> Result<()> {
    let (entries, warnings) = devices::list(repo_root)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if entries.is_empty() {
        println!(
            "{}",
            "No devices registered yet. Run `commitbook init` to register this one.".dimmed()
        );
    } else {
        println!("Devices ({}):", entries.len());
        let width = entries
            .iter()
            .map(|entry| entry.device.name.chars().count())
            .max()
            .unwrap_or(0);
        for entry in &entries {
            let marker = if entry.this_device { "*" } else { " " };
            let this = if entry.this_device {
                "  (this device)".dimmed().to_string()
            } else {
                String::new()
            };
            println!(
                "  {marker} {:<width$}  {:<8}  {}{this}",
                entry.device.name,
                entry.device.platform,
                entry.id.dimmed(),
            );
        }
    }
    for warning in warnings {
        println!("  {} {warning}", "WARN".yellow().bold());
    }
    Ok(())
}

/// Rename this device. The change is committed by the next sync.
pub fn rename(repo_root: &Path, name: &str) -> Result<()> {
    let _lock = RepoLock::acquire(repo_root)?;
    devices::rename(repo_root, name)?;
    println!(
        "  {} Renamed this device to {}. The next sync shares the change.",
        "OK".green().bold(),
        name.trim().cyan()
    );
    Ok(())
}

/// Remove another device. The change is committed by the next sync.
pub fn remove(repo_root: &Path, id: &str) -> Result<()> {
    let _lock = RepoLock::acquire(repo_root)?;
    devices::remove(repo_root, id)?;
    println!(
        "  {} Removed device {id}. The next sync shares the change.",
        "OK".green().bold()
    );
    Ok(())
}
