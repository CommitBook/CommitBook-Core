mod commands;
mod errors;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use colored::Colorize;

/// CommitBook, Markdown workspace with git sync.
#[derive(Parser)]
#[command(
    name = "commitbook",
    version,
    about,
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Output as JSON (for status, preview, devices, doctor, log)
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum TokenAction {
    /// Store a token in .CommitBook/local/auth.toml. The token is read from a
    /// hidden prompt, or from stdin when piped (`echo "$T" | commitbook token
    /// set`), never from an argument: arguments end up in shell history and
    /// `ps` output.
    Set {
        /// Provider name for the stored token (github, gitlab, etc.)
        #[arg(long)]
        provider: Option<String>,
    },
    /// Remove the stored token
    Clear,
}

#[derive(Subcommand)]
enum DevicesAction {
    /// Rename this device
    Rename {
        /// New name, e.g. "Work laptop"
        name: String,
    },
    /// Remove another device, e.g. a retired computer
    Remove {
        /// Device id, as shown by `commitbook devices`
        id: String,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize CommitBook in the current git repo
    Init {
        /// Skip the confirmation prompt before committing and pushing
        #[arg(short, long)]
        yes: bool,
    },

    /// Commit locally and sync with remote
    #[command(alias = "run")]
    Sync,

    /// Preview the next local snapshot without changing files or contacting the remote
    Preview,

    /// Start the sync scheduler
    Start,

    /// Stop the sync scheduler
    Stop,

    /// Show current sync state
    Status,

    /// List the devices syncing this CommitBook
    Devices {
        #[command(subcommand)]
        action: Option<DevicesAction>,
    },

    /// Change the sync schedule
    Schedule {
        /// Schedule: "15m", "1h", "4h", "daily", or a cron expression
        expression: String,
    },

    /// Check system health and dependencies
    Doctor {
        /// Attempt to auto-repair common issues (scheduler installation and
        /// missing logs directory). Diagnostic-only without this flag.
        #[arg(long)]
        fix: bool,
    },

    /// Show recent activity log
    Log {
        /// Number of recent entries to show
        #[arg(short = 'n', long, default_value = "20")]
        lines: usize,

        /// Stream new entries as they're appended (Ctrl-C to stop)
        #[arg(short = 'f', long)]
        tail: bool,
    },

    /// Manage the optional token for token-backed transports
    #[command(hide = true)]
    Token {
        #[command(subcommand)]
        action: TokenAction,
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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let verbose = cli.verbose;

    if let Err(e) = run(cli).await {
        if verbose {
            eprintln!("{} {:#}", "ERROR".red().bold(), e);
        } else {
            eprintln!("{} {}", "ERROR".red().bold(), errors::humanize(&e));
            eprintln!("  {}", "Run with --verbose for the full error.".dimmed());
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
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
            clap_mangen::Man::new(Cli::command()).render(&mut std::io::stdout())?;
            return Ok(());
        }
        Commands::Init { yes } => {
            return commands::init_cmd::run_init(*yes);
        }
        _ => {}
    }

    // All remaining commands require an initialized .CommitBook/.
    let cb_dir = commitbook_engine::state::ensure_initialized()?;
    let repo_root = commitbook_engine::state::repo_root(&cb_dir);

    match cli.command {
        Commands::Preview => commands::preview::run(&repo_root, cli.json)?,
        Commands::Sync => commands::sync_cmd::run_sync(&repo_root).await?,
        Commands::Start => commands::start::run(&cb_dir, &repo_root)?,
        Commands::Stop => commands::stop::run(&cb_dir, &repo_root)?,
        Commands::Status => commands::status::run(&cb_dir, &repo_root, cli.json)?,
        Commands::Devices { action } => match action {
            None => commands::devices::list(&repo_root, cli.json)?,
            Some(DevicesAction::Rename { name }) => commands::devices::rename(&repo_root, &name)?,
            Some(DevicesAction::Remove { id }) => commands::devices::remove(&repo_root, &id)?,
        },
        Commands::Schedule { expression } => {
            commands::schedule::run(&cb_dir, &repo_root, &expression)?;
        }
        Commands::Doctor { fix } => commands::doctor::run(&cb_dir, &repo_root, cli.json, fix)?,
        Commands::Log { lines, tail } => {
            commands::log::run(&cb_dir, &repo_root, lines, cli.json, tail)?;
        }
        Commands::Token { action } => match action {
            TokenAction::Set { provider } => {
                commands::token::set(&cb_dir, &repo_root, provider).await?;
            }
            TokenAction::Clear => commands::token::clear(&cb_dir)?,
        },
        Commands::Init { .. } | Commands::Completions { .. } | Commands::Manpage => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
