mod app;
mod ui;

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

fn parse_args() -> Result<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let mut repo_path: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" => {
                i += 1;
                if i >= args.len() {
                    bail!("--repo requires a path argument");
                }
                repo_path = Some(PathBuf::from(&args[i]));
            }
            _ => bail!("Unknown argument: {}", args[i]),
        }
        i += 1;
    }

    commitbook_core::config::resolve_repo_path(repo_path.as_deref())
}

fn main() -> Result<()> {
    let repo_path = parse_args()?;

    // Verify the repo has CommitBook initialized
    if !commitbook_core::config::local::LocalConfig::exists(&repo_path) {
        bail!(
            "CommitBook not initialized in {}. Run `commitbook init` first.",
            repo_path.display()
        );
    }

    // Set up terminal
    crossterm::terminal::enable_raw_mode().context("Failed to enable raw mode")?;
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

    // Restore terminal
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
