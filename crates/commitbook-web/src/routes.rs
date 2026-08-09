use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use axum::Json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use askama::Template;

use commitbook_engine::ai::ProviderChain;
use commitbook_engine::config::local::LocalConfig;
use commitbook_engine::cron;
use commitbook_engine::git::GitRepo;
use commitbook_engine::logger::FileLogger;

use crate::models::*;

pub struct AppState {
    pub repo_path: PathBuf,
}

// ---------------------------------------------------------------------------
// Template structs
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    running: bool,
    schedule_desc: String,
    current_branch: String,
    auto_push: bool,
    last_commit: String,
    changes_total: usize,
    changes_summary: String,
    providers: Vec<ProviderInfo>,
}

#[derive(Template)]
#[template(path = "logs.html")]
struct LogsTemplate {}

#[derive(Template)]
#[template(path = "config.html")]
struct ConfigTemplate {
    schedule: String,
    branch: String,
    auto_push: bool,
}

#[derive(Template)]
#[template(path = "partials/status.html")]
struct StatusPartial {
    running: bool,
    schedule_desc: String,
    current_branch: String,
    auto_push: bool,
    last_commit: String,
    changes_total: usize,
    changes_summary: String,
}

#[derive(Template)]
#[template(path = "partials/providers.html")]
struct ProvidersPartial {
    providers: Vec<ProviderInfo>,
}

#[derive(Template)]
#[template(path = "partials/logs.html")]
struct LogsPartial {
    entries: Vec<LogEntry>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn load_status(repo_path: &Path) -> StatusResponse {
    let config = LocalConfig::load(repo_path).ok();
    let running = cron::is_loaded(repo_path);

    let (schedule, schedule_desc, auto_push, branch, last_commit) = match &config {
        Some(c) => (
            c.schedule.clone(),
            cron::describe_schedule(&c.schedule),
            c.git.auto_push,
            c.git.branch.clone(),
            None::<String>, // last_commit moved to state.toml
        ),
        None => (String::new(), "Unknown".into(), true, "main".into(), None),
    };

    let (current_branch, changes_total, changes_summary) =
        if let Ok(repo) = GitRepo::open(repo_path) {
            let branch = repo.current_branch().unwrap_or_else(|_| "unknown".into());
            let changes = repo.changes_summary().unwrap_or_default();
            let total = changes.total();
            let summary = changes.to_summary_text();
            (branch, total, summary)
        } else {
            ("unknown".into(), 0, "unknown".into())
        };

    StatusResponse {
        running,
        enabled: config.as_ref().map(|c| c.enabled).unwrap_or(false),
        schedule,
        schedule_desc,
        branch,
        current_branch,
        auto_push,
        last_commit,
        changes_total,
        changes_summary,
    }
}

fn load_providers() -> Vec<ProviderInfo> {
    let chain = ProviderChain::new();
    let default_keys = vec![
        "gh-copilot".to_string(),
        "claude-cli".to_string(),
        "codex-cli".to_string(),
    ];
    chain
        .check_availability(&default_keys)
        .into_iter()
        .map(|(key, name, available)| ProviderInfo {
            key,
            name,
            available,
        })
        .collect()
}

fn load_log_entries(repo_path: &Path, limit: usize, offset: usize) -> Vec<LogEntry> {
    load_log_entries_filtered(repo_path, limit, offset, None)
}

fn load_log_entries_filtered(
    repo_path: &Path,
    limit: usize,
    offset: usize,
    level: Option<&str>,
) -> Vec<LogEntry> {
    let logger = match FileLogger::new(repo_path, 30) {
        Ok(l) => l,
        Err(_) => return Vec::new(),
    };

    let lines = logger
        .read_entries_filtered(limit, offset, level)
        .unwrap_or_default();
    lines
        .iter()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(LogEntry {
                timestamp: v["ts"].as_str().unwrap_or("").to_string(),
                level: v["level"].as_str().unwrap_or("INFO").to_string(),
                message: v["msg"].as_str().unwrap_or("").to_string(),
                provider: v["provider"].as_str().map(|s| s.to_string()),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// HTML page handlers
// ---------------------------------------------------------------------------

pub async fn dashboard(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = load_status(&state.repo_path);
    let providers = load_providers();

    let tpl = DashboardTemplate {
        running: status.running,
        schedule_desc: status.schedule_desc,
        current_branch: status.current_branch,
        auto_push: status.auto_push,
        last_commit: status.last_commit.unwrap_or_else(|| "never".into()),
        changes_total: status.changes_total,
        changes_summary: status.changes_summary,
        providers,
    };
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

pub async fn logs_page(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let tpl = LogsTemplate {};
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

pub async fn config_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let config = LocalConfig::load(&state.repo_path).ok();
    let (schedule, branch, auto_push) = match config {
        Some(c) => (c.schedule, c.git.branch, c.git.auto_push),
        None => ("0 * * * *".into(), "main".into(), true),
    };

    let tpl = ConfigTemplate {
        schedule,
        branch,
        auto_push,
    };
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

// ---------------------------------------------------------------------------
// HTMX partial handlers
// ---------------------------------------------------------------------------

pub async fn htmx_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = load_status(&state.repo_path);
    let tpl = StatusPartial {
        running: status.running,
        schedule_desc: status.schedule_desc,
        current_branch: status.current_branch,
        auto_push: status.auto_push,
        last_commit: status.last_commit.unwrap_or_else(|| "never".into()),
        changes_total: status.changes_total,
        changes_summary: status.changes_summary,
    };
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

pub async fn htmx_providers(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let providers = load_providers();
    let tpl = ProvidersPartial { providers };
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

pub async fn htmx_logs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let entries = load_log_entries(&state.repo_path, 50, 0);
    let tpl = LogsPartial { entries };
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

// ---------------------------------------------------------------------------
// REST API handlers
// ---------------------------------------------------------------------------

pub async fn api_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    Json(load_status(&state.repo_path))
}

pub async fn api_logs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LogsQuery>,
) -> Json<LogsResponse> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    let level = query.level.as_deref();

    // Count total matching entries (no limit/offset applied).
    let total = load_log_entries_filtered(&state.repo_path, usize::MAX, 0, level).len();
    // Fetch the requested page.
    let entries = load_log_entries_filtered(&state.repo_path, limit, offset, level);

    Json(LogsResponse {
        entries,
        total,
        offset,
        limit,
    })
}

pub async fn api_config(
    State(state): State<Arc<AppState>>,
    Json(update): Json<ConfigUpdate>,
) -> impl IntoResponse {
    let result = (|| -> anyhow::Result<()> {
        let mut config = LocalConfig::load(&state.repo_path)?;

        if let Some(schedule) = &update.schedule {
            cron::validate_cron_expression(schedule)?;
            config.schedule = schedule.clone();
        }
        if let Some(auto_push) = update.auto_push {
            config.git.auto_push = auto_push;
        }
        if let Some(branch) = &update.branch {
            config.git.branch = branch.clone();
        }

        config.save(&state.repo_path)?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            Html(r#"<div class="flash flash-success">Configuration saved.</div>"#.to_string())
        }
        Err(e) => {
            let msg = escape_html(&e.to_string());
            Html(format!(
                r#"<div class="flash flash-error">Error: {}</div>"#,
                msg
            ))
        }
    }
}

pub async fn api_start(State(state): State<Arc<AppState>>) -> Json<ActionResponse> {
    let result = (|| -> anyhow::Result<String> {
        let config = LocalConfig::load(&state.repo_path)?;
        let commitbook_bin = which::which("commitbook").unwrap_or_else(|_| {
            let bin = std::env::current_exe().expect("cannot determine current exe");
            bin.parent().map(|p| p.join("commitbook")).unwrap_or(bin)
        });
        cron::install(&state.repo_path, &config.schedule, &commitbook_bin)
    })();

    match result {
        Ok(_) => Json(ActionResponse {
            success: true,
            message: "Scheduler started".into(),
        }),
        Err(e) => Json(ActionResponse {
            success: false,
            message: format!("Failed to start: {}", e),
        }),
    }
}

pub async fn api_stop(State(state): State<Arc<AppState>>) -> Json<ActionResponse> {
    match cron::uninstall(&state.repo_path, None) {
        Ok(()) => Json(ActionResponse {
            success: true,
            message: "Scheduler stopped".into(),
        }),
        Err(e) => Json(ActionResponse {
            success: false,
            message: format!("Failed to stop: {}", e),
        }),
    }
}

pub async fn api_providers() -> Json<Vec<ProviderInfo>> {
    Json(load_providers())
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;
