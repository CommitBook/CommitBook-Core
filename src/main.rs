mod ai;
mod commands;
mod config;
mod cron;
mod git;
mod logger;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::env;
use std::path::PathBuf;

/// CommitBook - Automated git commits for your markdown notebooks.
#[derive(Parser)]
#[command(name = "commitbook", version, about, long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Path to the git repository (defaults to current directory)
    #[arg(long, global = true)]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize CommitBook in a git repository
    Setup,

    /// Check system health and dependencies
    Doctor,

    /// Start auto-commits for this repository
    Start,

    /// Stop auto-commits for this repository
    Stop,

    /// Change the commit schedule
    #[command(name = "set-schedule")]
    SetSchedule {
        /// Schedule expression: "hourly", "daily", "every-4h", or a cron expression
        expression: String,
    },

    /// Run a single auto-commit cycle (used internally by cron)
    #[command(hide = true)]
    AutoCommit,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize env_logger based on verbosity flags
    if cli.verbose {
        env::set_var("RUST_LOG", "debug");
    } else if !cli.quiet {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    // Resolve the repository path
    let repo_path = match &cli.repo {
        Some(p) => p.clone(),
        None => env::current_dir()?,
    };

    match cli.command {
        Commands::Setup => commands::setup::run(&repo_path)?,
        Commands::Doctor => commands::doctor::run(&repo_path)?,
        Commands::Start => commands::start::run(&repo_path)?,
        Commands::Stop => commands::stop::run(&repo_path)?,
        Commands::SetSchedule { expression } => {
            commands::schedule::run(&repo_path, &expression)?
        }
        Commands::AutoCommit => commands::auto_commit::run(&repo_path).await?,
    }

    Ok(())
}
