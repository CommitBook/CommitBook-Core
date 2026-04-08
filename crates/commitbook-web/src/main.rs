mod models;
mod routes;

use anyhow::{bail, Context, Result};
use axum::routing::{get, post};
use axum::Router;
use std::path::PathBuf;
use std::sync::Arc;

use routes::AppState;

fn parse_args() -> Result<(PathBuf, u16)> {
    let args: Vec<String> = std::env::args().collect();
    let mut repo_path: Option<PathBuf> = None;
    let mut port: u16 = 9847;

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
            "--port" => {
                i += 1;
                if i >= args.len() {
                    bail!("--port requires a number");
                }
                port = args[i]
                    .parse()
                    .context("--port must be a valid port number")?;
            }
            _ => bail!("Unknown argument: {}", args[i]),
        }
        i += 1;
    }

    let repo = commitbook_core::config::resolve_repo_path(repo_path.as_deref())?;
    Ok((repo, port))
}

#[tokio::main]
async fn main() -> Result<()> {
    let (repo_path, port) = parse_args()?;

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
