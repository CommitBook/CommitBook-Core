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
use commitbook_engine::inspection::RepositoryStatus;
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
        .route("/changes", get(changes_page))
        .route("/conflicts", get(conflicts_page))
        .route("/api/changes", get(api_changes))
        .route("/api/conflicts", get(api_conflicts))
        .route("/api/conflicts/resolve", post(api_resolve))
        .route("/api/conflicts/proposal", post(api_proposal))
        .route("/api/sync", post(api_sync))
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
    status_lines: Vec<String>,
    needs_attention: bool,
    running: bool,
    schedule_desc: String,
    current_branch: String,
    auto_push: bool,
    providers: Vec<ProviderInfo>,
}

#[derive(Template)]
#[template(path = "logs.html")]
struct LogsTemplate {}

#[derive(Template)]
#[template(path = "config.html")]
struct ConfigTemplate {
    review_ai_resolutions: bool,
    resolver: String,
    ai_messages: bool,
    schedule: String,
    branch: String,
    auto_push: bool,
}

#[derive(Template)]
#[template(path = "partials/status.html")]
struct StatusPartial {
    status_lines: Vec<String>,
    needs_attention: bool,
    running: bool,
    schedule_desc: String,
    current_branch: String,
    auto_push: bool,
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
    let repository = RepositoryStatus::read(repo_path);
    StatusResponse {
        running: cron::is_loaded(repo_path),
        enabled: repository.enabled.unwrap_or(false),
        schedule: repository.schedule.clone().unwrap_or_default(),
        schedule_desc: repository
            .schedule
            .as_deref()
            .map(cron::describe_schedule)
            .unwrap_or_else(|| "Unknown".into()),
        branch: repository
            .branch
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        current_branch: repository
            .current_branch
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        auto_push: repository.auto_push.unwrap_or(false),
        last_commit: repository.last_commit.clone(),
        changes_total: repository.changes_total,
        changes_summary: repository.local_status.clone(),
        repository,
    }
}

fn load_providers(repo_path: &Path) -> Vec<ProviderInfo> {
    let ai_messages = LocalConfig::load_read_only(repo_path)
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
    let logger = FileLogger::read_only(repo_path, 30);

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

pub async fn dashboard(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let path = state.repo_path.clone();
    let status = blocking(move || load_status(&path)).await?;
    let path = state.repo_path.clone();
    let providers = blocking(move || load_providers(&path)).await?;

    let tpl = DashboardTemplate {
        status_lines: status.repository.lines(),
        needs_attention: status.repository.merge_in_progress
            || !status.repository.conflicts.is_empty(),
        running: status.running,
        schedule_desc: status.schedule_desc,
        current_branch: status.current_branch,
        auto_push: status.auto_push,
        providers,
    };
    Ok(Html(tpl.render().unwrap_or_else(|e| {
        format!("Template error: {}", escape_html(&e.to_string()))
    })))
}

pub async fn logs_page(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let tpl = LogsTemplate {};
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

pub async fn config_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let path = state.repo_path.clone();
    let config = match blocking(move || LocalConfig::load_read_only(&path)).await {
        Ok(Ok(config)) => config,
        Ok(Err(error)) => {
            return Html(format!(
                "Cannot load configuration: {}",
                escape_html(&format!("{error:#}"))
            ))
        }
        Err(_) => return Html("Configuration inspection failed".into()),
    };
    let tpl = ConfigTemplate {
        review_ai_resolutions: config.conflict.review_ai_resolutions,
        resolver: config.conflict.resolver,
        ai_messages: config.commit.ai_messages,
        schedule: config.schedule,
        branch: config.git.branch,
        auto_push: config.git.auto_push,
    };
    Html(
        tpl.render()
            .unwrap_or_else(|e| format!("Template error: {}", escape_html(&e.to_string()))),
    )
}

// ---------------------------------------------------------------------------
// HTMX partial handlers
// ---------------------------------------------------------------------------

pub async fn htmx_status(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let path = state.repo_path.clone();
    let status = blocking(move || load_status(&path)).await?;
    let tpl = StatusPartial {
        status_lines: status.repository.lines(),
        needs_attention: status.repository.merge_in_progress
            || !status.repository.conflicts.is_empty(),
        running: status.running,
        schedule_desc: status.schedule_desc,
        current_branch: status.current_branch,
        auto_push: status.auto_push,
    };
    Ok(Html(tpl.render().unwrap_or_else(|e| {
        format!("Template error: {}", escape_html(&e.to_string()))
    })))
}

pub async fn htmx_providers(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, StatusCode> {
    let path = state.repo_path.clone();
    let providers = blocking(move || load_providers(&path)).await?;
    let tpl = ProvidersPartial { providers };
    Ok(Html(tpl.render().unwrap_or_else(|e| {
        format!("Template error: {}", escape_html(&e.to_string()))
    })))
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

pub async fn api_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, StatusCode> {
    Ok(Json(blocking(move || load_status(&state.repo_path)).await?))
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
        review_ai_resolutions: update.review_ai_resolutions,
        resolver: update.resolver,
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

pub async fn api_providers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ProviderInfo>>, StatusCode> {
    Ok(Json(
        blocking(move || load_providers(&state.repo_path)).await?,
    ))
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;

#[derive(Template)]
#[template(path = "changes.html")]
struct ChangesTemplate {
    preview: commitbook_engine::inspection::CommitPreview,
}
#[derive(Template)]
#[template(path = "conflicts.html")]
struct ConflictsTemplate {}
pub async fn changes_page(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let preview =
        blocking(move || commitbook_engine::inspection::preview(&state.repo_path)).await?;
    Ok(Html(
        ChangesTemplate { preview }
            .render()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    ))
}
pub async fn conflicts_page() -> Result<Html<String>, StatusCode> {
    Ok(Html(
        ConflictsTemplate {}
            .render()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    ))
}
pub async fn api_changes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<commitbook_engine::inspection::CommitPreview>, StatusCode> {
    Ok(Json(
        blocking(move || commitbook_engine::inspection::preview(&state.repo_path)).await?,
    ))
}
fn review_error(error: anyhow::Error) -> Response {
    use commitbook_engine::review::ReviewError;
    let status = match error.downcast_ref::<ReviewError>() {
        Some(ReviewError::Stale) => StatusCode::CONFLICT,
        Some(ReviewError::Missing) => StatusCode::NOT_FOUND,
        None => mutation_error_status(&error),
    };
    action_json(status, false, format!("{error:#}"))
}
pub async fn api_conflicts(State(state): State<Arc<AppState>>) -> Result<Response, StatusCode> {
    Ok(
        match blocking(move || commitbook_engine::review::list(&state.repo_path)).await? {
            Ok(conflicts) => Json(conflicts).into_response(),
            Err(error) => review_error(error),
        },
    )
}
pub async fn api_resolve(
    State(state): State<Arc<AppState>>,
    Json(input): Json<commitbook_engine::review::ResolutionInput>,
) -> Result<Response, StatusCode> {
    Ok(
        match blocking(move || {
            let lock = commitbook_engine::state::RepoLock::acquire(&state.repo_path)?;
            let config = LocalConfig::load_read_only(&state.repo_path)?;
            commitbook_engine::review::apply_locked(
                &state.repo_path,
                &config.git.branch,
                &input,
                &lock,
            )
        })
        .await?
        {
            Ok(()) => action_json(
                StatusCode::OK,
                true,
                "Resolved locally. Sync to publish according to your auto-push setting.".into(),
            ),
            Err(error) => review_error(error),
        },
    )
}
pub async fn api_proposal(
    State(state): State<Arc<AppState>>,
    Json(input): Json<commitbook_engine::review::ResolutionInput>,
) -> Result<Response, StatusCode> {
    Ok(
        match blocking(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(commitbook_engine::review::proposal_action(
                    &state.repo_path,
                    &input,
                ))
        })
        .await?
        {
            Ok(()) => action_json(
                StatusCode::OK,
                true,
                "Proposal updated. Accepted resolutions are saved locally.".into(),
            ),
            Err(error) => review_error(error),
        },
    )
}
pub async fn api_sync(State(state): State<Arc<AppState>>) -> Result<Response, StatusCode> {
    let result = blocking(move || -> anyhow::Result<_> {
        let lock = commitbook_engine::state::RepoLock::acquire(&state.repo_path)?;
        let config = LocalConfig::load(&state.repo_path)?;
        let logger = FileLogger::new(&state.repo_path, config.logging.max_log_days)?;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let repo = commitbook_engine::git::GitRepo::open(&state.repo_path)?;
                let message = if config.commit.ai_messages
                    && !repo.merge_in_progress()
                    && repo.has_dirty_changes()?
                {
                    let chain = ProviderChain::new();
                    let keys =
                        ["gh-copilot", "claude-cli", "codex-cli", "fallback"].map(str::to_string);
                    Some(
                        chain
                            .generate(&repo.changes_summary()?, &keys, &state.repo_path)
                            .await
                            .0,
                    )
                } else {
                    None
                };
                commitbook_engine::sync::sync_repository_locked(
                    &state.repo_path,
                    &config,
                    &logger,
                    message,
                    &lock,
                )
                .await
            })
    })
    .await?;
    Ok(match result {
        Ok(outcome) => {
            let success = outcome.errors.is_empty() && outcome.manual_conflicts == 0;
            let message = if outcome.manual_conflicts > 0 {
                format!(
                    "{} conflicts need resolution or review. {}",
                    outcome.manual_conflicts,
                    outcome.errors.join("; ")
                )
            } else if !outcome.errors.is_empty() {
                outcome.errors.join("; ")
            } else {
                "Sync cycle complete. Check status for local and remote details.".into()
            };
            action_json(
                if success {
                    StatusCode::OK
                } else {
                    StatusCode::CONFLICT
                },
                success,
                message,
            )
        }
        Err(error) => review_error(error),
    })
}
