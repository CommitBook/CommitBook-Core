use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use http_body_util::BodyExt;
use std::process::Command as ProcessCommand;
use tower::ServiceExt;

fn git_init(path: &Path) {
    ProcessCommand::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .output()
        .expect("git init failed");
}

fn setup_test_app() -> (tempfile::TempDir, Router) {
    let tmp = tempfile::tempdir().unwrap();
    git_init(tmp.path());
    commitbook_core::config::local::LocalConfig::init(tmp.path(), "0 * * * *").unwrap();

    let state = Arc::new(AppState {
        repo_path: tmp.path().to_path_buf(),
    });

    let app = Router::new()
        .route("/", get(dashboard))
        .route("/logs", get(logs_page))
        .route("/config", get(config_page))
        .route("/htmx/status", get(htmx_status))
        .route("/htmx/providers", get(htmx_providers))
        .route("/htmx/logs", get(htmx_logs))
        .route("/api/status", get(api_status))
        .route("/api/logs", get(api_logs))
        .route("/api/config", post(api_config))
        .route("/api/start", post(api_start))
        .route("/api/stop", post(api_stop))
        .route("/api/providers", get(api_providers))
        .with_state(state);

    (tmp, app)
}

async fn get_json<T: serde::de::DeserializeOwned>(app: Router, uri: &str) -> (StatusCode, T) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let parsed: T = serde_json::from_slice(&body).unwrap();
    (status, parsed)
}

async fn post_json(app: Router, uri: &str, body: serde_json::Value) -> (StatusCode, String) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    (status, text)
}

#[tokio::test]
async fn test_api_status_returns_json() {
    let (_tmp, app) = setup_test_app();
    let (status, body): (_, StatusResponse) = get_json(app, "/api/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.running);
}

#[tokio::test]
async fn test_api_status_has_schedule() {
    let (_tmp, app) = setup_test_app();
    let (_, body): (_, StatusResponse) = get_json(app, "/api/status").await;
    assert_eq!(body.schedule, "0 * * * *");
    assert!(!body.schedule_desc.is_empty());
}

#[tokio::test]
async fn test_api_logs_empty() {
    let (_tmp, app) = setup_test_app();
    let (status, body): (_, LogsResponse) = get_json(app, "/api/logs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.total, 0);
    assert_eq!(body.limit, 50);
    assert_eq!(body.offset, 0);
}

#[tokio::test]
async fn test_api_logs_respects_limit() {
    let (tmp, app) = setup_test_app();

    // Write log entries
    let logs_dir = tmp.path().join(".CommitBook").join("logs");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_file = logs_dir.join(format!("{}.log", today));
    let mut entries = String::new();
    for i in 0..5 {
        entries.push_str(&format!(
            r#"{{"ts":"2026-04-09 10:0{}:00","level":"INFO","msg":"entry {}"}}"#,
            i, i
        ));
        entries.push('\n');
    }
    std::fs::write(&log_file, entries).unwrap();

    let (_, body): (_, LogsResponse) = get_json(app, "/api/logs?limit=2").await;
    assert_eq!(body.limit, 2);
    assert!(body.entries.len() <= 2);
}

#[tokio::test]
async fn test_api_logs_filters_by_level() {
    let (tmp, app) = setup_test_app();

    let logs_dir = tmp.path().join(".CommitBook").join("logs");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_file = logs_dir.join(format!("{}.log", today));
    let entries = concat!(
        r#"{"ts":"2026-04-09 10:00:00","level":"INFO","msg":"info msg"}"#, "\n",
        r#"{"ts":"2026-04-09 10:01:00","level":"ERROR","msg":"error msg"}"#, "\n",
        r#"{"ts":"2026-04-09 10:02:00","level":"INFO","msg":"info msg 2"}"#, "\n",
    );
    std::fs::write(&log_file, entries).unwrap();

    let (_, body): (_, LogsResponse) = get_json(app, "/api/logs?level=ERROR").await;
    assert!(body.entries.iter().all(|e| e.level == "ERROR"));
}

#[tokio::test]
async fn test_api_config_updates_schedule() {
    let (tmp, app) = setup_test_app();
    let (status, _) = post_json(
        app,
        "/api/config",
        serde_json::json!({"schedule": "*/5 * * * *"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let config = commitbook_core::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.schedule, "*/5 * * * *");
}

#[tokio::test]
async fn test_api_config_rejects_invalid_cron() {
    let (_tmp, app) = setup_test_app();
    let (_, body) = post_json(
        app,
        "/api/config",
        serde_json::json!({"schedule": "not a cron"}),
    )
    .await;
    assert!(body.contains("Error"), "got: {}", body);
}

#[tokio::test]
async fn test_api_config_partial_update() {
    let (tmp, app) = setup_test_app();
    let (status, _) = post_json(
        app,
        "/api/config",
        serde_json::json!({"auto_push": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let config = commitbook_core::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert!(!config.git.auto_push);
    assert_eq!(config.schedule, "0 * * * *"); // unchanged
}

#[tokio::test]
async fn test_api_providers_returns_list() {
    let (_tmp, app) = setup_test_app();
    let (status, body): (_, Vec<ProviderInfo>) = get_json(app, "/api/providers").await;
    assert_eq!(status, StatusCode::OK);
    // Should return at least one provider entry (even if unavailable)
    assert!(!body.is_empty());
}

#[tokio::test]
async fn test_api_stop_returns_action_response() {
    let (_tmp, app) = setup_test_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let action: ActionResponse = serde_json::from_slice(&body).unwrap();
    assert!(!action.message.is_empty());
}

#[tokio::test]
async fn test_dashboard_returns_html() {
    let (_tmp, app) = setup_test_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
