mod models;
mod routes;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

use routes::AppState;

/// CommitBook Web, Browser dashboard for monitoring CommitBook.
#[derive(Parser)]
#[command(name = "commitbook-web", version, about)]
struct Cli {
    /// Path to the git repository (defaults to current directory)
    #[arg(long)]
    repo: Option<PathBuf>,

    /// Port for the web server
    #[arg(long, default_value = "9847")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_path = match cli.repo {
        Some(p) => std::fs::canonicalize(&p).unwrap_or(p),
        None => std::env::current_dir().context("Cannot determine current directory")?,
    };
    let port = cli.port;

    if !commitbook_engine::config::local::LocalConfig::exists(&repo_path) {
        bail!(
            "{} ({})",
            commitbook_engine::state::NOT_INITIALIZED_MESSAGE,
            repo_path.display()
        );
    }

    let state = Arc::new(AppState::new(repo_path));
    let app = routes::build_router(state);

    let addr = format!("127.0.0.1:{}", port);
    println!("CommitBook Web: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    axum::serve(listener, app).await.context("Server error")?;

    Ok(())
}
