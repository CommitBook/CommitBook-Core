mod app;
mod ui;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;

/// CommitBook TUI — Terminal dashboard for monitoring CommitBook.
#[derive(Parser)]
#[command(name = "commitbook-tui", version, about)]
struct Cli {
    /// Path to the git repository (defaults to current directory)
    #[arg(long)]
    repo: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_path = match cli.repo {
        Some(p) => std::fs::canonicalize(&p).unwrap_or(p),
        None => std::env::current_dir().context("Cannot determine current directory")?,
    };

    // Verify the repo has CommitBook initialized
    if !commitbook_engine::config::local::LocalConfig::exists(&repo_path) {
        bail!(
            "CommitBook not initialized in {}. Run `commitbook sync` first.",
            repo_path.display()
        );
    }

    // RAII guard ensures terminal is restored even on panic or early return
    struct TerminalGuard;
    impl Drop for TerminalGuard {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            );
        }
    }

    // Set up terminal
    crossterm::terminal::enable_raw_mode().context("Failed to enable raw mode")?;
    let _guard = TerminalGuard;

    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )
    .context("Failed to enter alternate screen")?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend).context("Failed to create terminal")?;

    let result = app::run(&mut terminal, &repo_path);

    // Restore terminal (happy path — guard handles failure/panic paths)
    crossterm::terminal::disable_raw_mode().ok();
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();

    result
}
