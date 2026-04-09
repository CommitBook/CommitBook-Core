use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

/// Initialize the CommitBook database and global config directory.
pub fn run() -> Result<()> {
    let config_dir = commitbook_dir()?;
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create {}", config_dir.display()))?;

    let db_path = config_dir.join("state.db");
    if db_path.exists() {
        println!(
            "{}",
            "CommitBook already initialized.".yellow()
        );
        println!("  Database: {}", db_path.display());
        return Ok(());
    }

    // Open the database — this triggers migrations and creates all tables.
    let _conn = commitbook_core::storage::db::open_database(&db_path)?;

    println!("{}", "CommitBook initialized.".green().bold());
    println!("  Database: {}", db_path.display());
    println!();
    println!("Next steps:");
    println!("  commitbook workspace add --existing-repo /path/to/repo");
    println!("  commitbook login pat");

    Ok(())
}

/// Get the CommitBook config directory (~/.commitbook).
pub fn commitbook_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".commitbook"))
}

/// Get the database path.
pub fn db_path() -> Result<PathBuf> {
    Ok(commitbook_dir()?.join("state.db"))
}
