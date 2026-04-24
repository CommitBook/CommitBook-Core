mod commands;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use colored::Colorize;

/// CommitBook — Markdown workspace with git sync.
#[derive(Parser)]
#[command(name = "commitbook", version, about, long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Output as JSON (for status, doctor, log)
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize CommitBook in the current git repo
    Init,

    /// Commit locally and sync with remote
    Sync,

    /// Start the sync scheduler
    Start,

    /// Stop the sync scheduler
    Stop,

    /// Show current sync state
    Status,

    /// Change the sync schedule
    Schedule {
        /// Schedule: "hourly", "daily", "every-4h", or a cron expression
        expression: String,
    },

    /// Check system health and dependencies
    Doctor,

    /// Show open merge conflicts
    Conflicts,

    /// Show recent activity log
    Log {
        /// Number of recent entries to show
        #[arg(short = 'n', long, default_value = "20")]
        lines: usize,
    },

    /// Authenticate with a provider
    Login {
        /// Personal access token (for any provider)
        #[arg(long)]
        token: Option<String>,

        /// Provider name (github, gitlab, etc.)
        #[arg(long)]
        provider: Option<String>,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Generate man page
    #[command(hide = true)]
    Manpage,

    /// Run a single sync cycle (used internally by scheduler)
    #[command(hide = true)]
    Run,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    } else if !cli.quiet {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    // Commands that don't require .CommitBook/ to be initialized.
    match &cli.command {
        Commands::Completions { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "commitbook",
                &mut std::io::stdout(),
            );
            return Ok(());
        }
        Commands::Manpage => {
            clap_mangen::Man::new(Cli::command())
                .render(&mut std::io::stdout())?;
            return Ok(());
        }
        Commands::Init => {
            return commands::init_cmd::run_init();
        }
        _ => {}
    }

    // All remaining commands require an initialized .CommitBook/.
    let cb_dir = match commitbook_core::state::ensure_initialized() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("{} {}", "ERROR".red().bold(), e);
            std::process::exit(1);
        }
    };
    let repo_root = commitbook_core::state::repo_root(&cb_dir);

    match cli.command {
        Commands::Sync => commands::sync_cmd::run_sync(&cb_dir, &repo_root).await?,
        Commands::Start => commands::start::run(&cb_dir, &repo_root)?,
        Commands::Stop => commands::stop::run(&cb_dir, &repo_root)?,
        Commands::Status => commands::status::run(&cb_dir, &repo_root, cli.json)?,
        Commands::Schedule { expression } => {
            commands::schedule::run(&cb_dir, &repo_root, &expression)?;
        }
        Commands::Doctor => commands::doctor::run(&cb_dir, &repo_root, cli.json)?,
        Commands::Conflicts => commands::conflicts::run(&cb_dir, &repo_root)?,
        Commands::Log { lines } => {
            commands::log::run(&cb_dir, &repo_root, lines, cli.json)?;
        }
        Commands::Login { token, provider } => {
            commands::login::run(&cb_dir, &repo_root, token, provider).await?;
        }
        Commands::Run => {
            commands::sync_cmd::run_scheduled(&cb_dir, &repo_root).await?;
        }
        Commands::Init | Commands::Completions { .. } | Commands::Manpage => unreachable!(),
    }

    Ok(())
}
