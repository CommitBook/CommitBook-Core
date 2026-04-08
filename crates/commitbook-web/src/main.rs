mod models;
mod routes;

use anyhow::{bail, Context, Result};
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

use routes::AppState;

/// CommitBook Web — Browser dashboard for monitoring CommitBook.
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
    let repo_path = commitbook_core::config::resolve_repo_path(cli.repo.as_deref())?;
    let port = cli.port;

    if !commitbook_core::config::local::LocalConfig::exists(&repo_path) {
        bail!(
            "CommitBook not initialized in {}. Run `commitbook init` first.",
            repo_path.display()
        );
    }

    let state = Arc::new(AppState { repo_path });

    let app = Router::new()
        // HTML pages
        .route("/", get(routes::dashboard))
        .route("/logs", get(routes::logs_page))
        .route("/config", get(routes::config_page))
        // HTMX partials
        .route("/htmx/status", get(routes::htmx_status))
        .route("/htmx/providers", get(routes::htmx_providers))
        .route("/htmx/logs", get(routes::htmx_logs))
        // REST API
        .route("/api/status", get(routes::api_status))
        .route("/api/logs", get(routes::api_logs))
        .route("/api/config", post(routes::api_config))
        .route("/api/start", post(routes::api_start))
        .route("/api/stop", post(routes::api_stop))
        .route("/api/providers", get(routes::api_providers))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    println!("CommitBook Web: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    axum::serve(listener, app)
        .await
        .context("Server error")?;

    Ok(())
}
