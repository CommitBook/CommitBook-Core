mod commands;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

/// CommitBook — Automated git commits for your markdown notebooks.
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

    /// Output as JSON (for status, doctor, log)
    #[arg(long, global = true)]
    json: bool,

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

    /// Show current CommitBook state for this repository
    Status,

    /// Change the commit schedule
    #[command(name = "set-schedule")]
    SetSchedule {
        /// Schedule: "hourly", "daily", "every-4h", or a cron expression
        expression: String,
    },

    /// Trigger a manual commit cycle
    Commit {
        /// Show what would be committed without committing
        #[arg(long)]
        dry_run: bool,

        /// Override the AI-generated commit message
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Show recent auto-commit log entries
    Log {
        /// Number of recent entries to show
        #[arg(short = 'n', long, default_value = "20")]
        lines: usize,
    },

    /// Run a single auto-commit cycle (used internally by scheduler)
    #[command(hide = true)]
    AutoCommit,

    /// Remove CommitBook from this repository
    Uninstall {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        unsafe { std::env::set_var("RUST_LOG", "debug") };
    } else if !cli.quiet {
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    env_logger::init();

    let repo_path = commitbook_core::config::resolve_repo_path(cli.repo.as_deref())?;

    match cli.command {
        Commands::Setup => commands::setup::run(&repo_path)?,
        Commands::Doctor => commands::doctor::run(&repo_path, cli.json)?,
        Commands::Start => commands::start::run(&repo_path)?,
        Commands::Stop => commands::stop::run(&repo_path)?,
        Commands::Status => commands::status::run(&repo_path, cli.json)?,
        Commands::SetSchedule { expression } => {
            commands::schedule::run(&repo_path, &expression)?;
        }
        Commands::Commit { dry_run, message } => {
            commands::commit::run(&repo_path, dry_run, message.as_deref()).await?;
        }
        Commands::Log { lines } => commands::log::run(&repo_path, lines, cli.json)?,
        Commands::AutoCommit => commands::auto_commit::run(&repo_path).await?,
        Commands::Uninstall { force } => commands::uninstall::run(&repo_path, force)?,
        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "commitbook",
                &mut std::io::stdout(),
            );
        }
    }

    Ok(())
}
