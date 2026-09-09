use axum::extract::{FromRequest, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Form;
use axum::Json;
use axum::Router;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use askama::Template;

use commitbook_engine::ai::ProviderChain;
use commitbook_engine::config::local::LocalConfig;
use commitbook_engine::cron::{self, SchedulerAdapter, SystemScheduler};
use commitbook_engine::git::GitRepo;
use commitbook_engine::logger::FileLogger;
use commitbook_engine::settings::{self, SchedulerContext, SettingsUpdate, SettingsUpdateError};
use commitbook_engine::state::RepoLockContended;

use crate::models::*;

pub struct AppState {
    pub repo_path: PathBuf,
    /// Scheduler used by start/stop and settings updates. Tests inject a fake.
    pub scheduler: Arc<dyn SchedulerAdapter>,
    /// Binary the scheduler job should run.
    pub binary: PathBuf,
}

impl AppState {
    pub fn new(repo_path: PathBuf) -> Self {
        Self::with_scheduler(
            repo_path,
            Arc::new(SystemScheduler),
            settings::current_binary(),
        )
    }

    pub fn with_scheduler(
        repo_path: PathBuf,
        scheduler: Arc<dyn SchedulerAdapter>,
        binary: PathBuf,
    ) -> Self {
        Self {
            repo_path,
            scheduler,
            binary,
        }
    }
}

/// Build the full router. `main` and the tests share this so routes cannot
/// drift between the binary and its test harness.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        // HTML pages
        .route("/", get(dashboard))
        .route("/logs", get(logs_page))
        .route("/config", get(config_page))
        // HTMX partials
        .route("/htmx/status", get(htmx_status))
        .route("/htmx/providers", get(htmx_providers))
        .route("/htmx/logs", get(htmx_logs))
        // REST API
        .route("/api/status", get(api_status))
        .route("/api/logs", get(api_logs))
        .route("/api/config", post(api_config))
        .route("/api/start", post(api_start))
        .route("/api/stop", post(api_stop))
        .route("/api/providers", get(api_providers))
        .with_state(state)
}

/// Run repository, lock, or scheduler work off the async executor.
async fn blocking<T, F>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// HTTP status for an engine error surfaced by a mutation endpoint.
fn mutation_error_status(error: &anyhow::Error) -> StatusCode {
    if error.downcast_ref::<RepoLockContended>().is_some() {
        StatusCode::CONFLICT
    } else if error.downcast_ref::<SettingsUpdateError>().is_some() {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::BAD_REQUEST
    }
}

fn action_json(status: StatusCode, success: bool, message: String) -> Response {
    (status, Json(ActionResponse { success, message })).into_response()
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
    ai_messages: bool,
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

fn load_providers(repo_path: &Path) -> Vec<ProviderInfo> {
    let ai_messages = LocalConfig::load(repo_path)
        .map(|config| config.commit.ai_messages)
        .unwrap_or(false);
    if !ai_messages {
        return Vec::new();
    }
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
    let providers = load_providers(&state.repo_path);

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
    let (schedule, branch, auto_push, ai_messages) = match config {
        Some(c) => (
            c.schedule,
            c.git.branch,
            c.git.auto_push,
            c.commit.ai_messages,
        ),
        None => ("0 * * * *".into(), "main".into(), true, false),
    };

    let tpl = ConfigTemplate {
        ai_messages,
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

pub async fn htmx_providers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let providers = load_providers(&state.repo_path);
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

pub async fn api_config(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let is_form = request
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| {
            value
                .trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        });
    let update = if is_form {
        match Form::<ConfigUpdate>::from_request(request, &state).await {
            Ok(Form(update)) => update,
            Err(error) => return error.into_response(),
        }
    } else {
        match Json::<ConfigUpdate>::from_request(request, &state).await {
            Ok(Json(update)) => update,
            Err(error) => return error.into_response(),
        }
    };
    let settings_update = SettingsUpdate {
        schedule: update.schedule,
        branch: update.branch,
        auto_push: update.auto_push,
        ai_messages: update.ai_messages,
        enabled: None,
    };

    let repo_path = state.repo_path.clone();
    let scheduler = Arc::clone(&state.scheduler);
    let binary = state.binary.clone();
    let result = blocking(move || {
        let context = SchedulerContext::new(scheduler.as_ref(), binary);
        settings::update_settings(&repo_path, &settings_update, &context)
    })
    .await;

    let (status, success, message) = match result {
        Ok(Ok(outcome)) => {
            let mut message = "Configuration saved.".to_string();
            if outcome.scheduler_reinstalled {
                message.push_str(" Scheduler reinstalled with the new schedule.");
            }
            (StatusCode::OK, true, message)
        }
        Ok(Err(error)) => (
            mutation_error_status(&error),
            false,
            format!("Error: {error}"),
        ),
        Err(status) => (
            status,
            false,
            "Error: configuration task failed".to_string(),
        ),
    };

    if is_form {
        // htmx swaps the flash into the page; it ignores non-2xx responses, so
        // form submissions always answer 200 and carry the outcome in the body.
        let class = if success {
            "flash-success"
        } else {
            "flash-error"
        };
        return Html(format!(
            r#"<div class="flash {}">{}</div>"#,
            class,
            escape_html(&message)
        ))
        .into_response();
    }
    action_json(status, success, message)
}

async fn scheduler_action<F>(state: &AppState, action: F) -> Result<anyhow::Result<()>, StatusCode>
where
    F: FnOnce(&Path, &SchedulerContext<'_>) -> anyhow::Result<()> + Send + 'static,
{
    let repo_path = state.repo_path.clone();
    let scheduler = Arc::clone(&state.scheduler);
    let binary = state.binary.clone();
    blocking(move || {
        let context = SchedulerContext::new(scheduler.as_ref(), binary);
        action(&repo_path, &context)
    })
    .await
}

fn scheduler_action_response(
    result: Result<anyhow::Result<()>, StatusCode>,
    success_message: &str,
    failure_prefix: &str,
) -> Response {
    match result {
        Ok(Ok(())) => action_json(StatusCode::OK, true, success_message.to_string()),
        Ok(Err(error)) => {
            let status = if error.downcast_ref::<RepoLockContended>().is_some() {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            action_json(status, false, format!("{failure_prefix}: {error:#}"))
        }
        Err(status) => action_json(status, false, format!("{failure_prefix}: task failed")),
    }
}

pub async fn api_start(State(state): State<Arc<AppState>>) -> Response {
    let result = scheduler_action(&state, |repo_path, context| {
        settings::start_scheduler(repo_path, context).map(|_| ())
    })
    .await;
    scheduler_action_response(result, "Scheduler started", "Failed to start")
}

pub async fn api_stop(State(state): State<Arc<AppState>>) -> Response {
    let result = scheduler_action(&state, settings::stop_scheduler).await;
    scheduler_action_response(result, "Scheduler stopped", "Failed to stop")
}

pub async fn api_providers(State(state): State<Arc<AppState>>) -> Json<Vec<ProviderInfo>> {
    Json(load_providers(&state.repo_path))
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;
