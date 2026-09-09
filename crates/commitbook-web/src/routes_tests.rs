use super::*;
use axum::body::Body;
use axum::http::Request;
use commitbook_engine::cron::FakeScheduler;
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
    setup_test_app_with(Arc::new(FakeScheduler::stopped()))
}

fn setup_test_app_with(scheduler: Arc<FakeScheduler>) -> (tempfile::TempDir, Router) {
    let tmp = tempfile::tempdir().unwrap();
    git_init(tmp.path());
    commitbook_engine::config::local::LocalConfig::init(tmp.path(), "0 * * * *").unwrap();

    let state = Arc::new(AppState::with_scheduler(
        tmp.path().to_path_buf(),
        scheduler,
        PathBuf::from("/opt/commitbook/bin/commitbook"),
    ));

    (tmp, build_router(state))
}

async fn get_json<T: serde::de::DeserializeOwned>(app: Router, uri: &str) -> (StatusCode, T) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
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
    let logs_dir = tmp.path().join(".CommitBook").join("local").join("logs");
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

    let logs_dir = tmp.path().join(".CommitBook").join("local").join("logs");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_file = logs_dir.join(format!("{}.log", today));
    let entries = concat!(
        r#"{"ts":"2026-04-09 10:00:00","level":"INFO","msg":"info msg"}"#,
        "\n",
        r#"{"ts":"2026-04-09 10:01:00","level":"ERROR","msg":"error msg"}"#,
        "\n",
        r#"{"ts":"2026-04-09 10:02:00","level":"INFO","msg":"info msg 2"}"#,
        "\n",
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

    let config = commitbook_engine::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.schedule, "*/5 * * * *");
}

#[tokio::test]
async fn test_api_config_rejects_invalid_cron() {
    let (tmp, app) = setup_test_app();
    let (status, body) = post_json(
        app,
        "/api/config",
        serde_json::json!({"schedule": "not a cron"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let action: ActionResponse = serde_json::from_str(&body).unwrap();
    assert!(!action.success);
    assert!(action.message.contains("Error"), "got: {}", body);
    let config = commitbook_engine::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.schedule, "0 * * * *");
}

#[tokio::test]
async fn config_update_reinstalls_running_scheduler() {
    let fake = Arc::new(FakeScheduler::running("0 * * * *"));
    let (tmp, app) = setup_test_app_with(Arc::clone(&fake));
    let (status, body) = post_json(
        app,
        "/api/config",
        serde_json::json!({"schedule": "*/15 * * * *"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Scheduler reinstalled"), "got: {body}");
    assert_eq!(fake.installed_schedule().as_deref(), Some("*/15 * * * *"));
    assert!(fake.calls().iter().any(|call| matches!(
        call,
        commitbook_engine::cron::fake::FakeCall::Install { binary, .. }
            if binary == &PathBuf::from("/opt/commitbook/bin/commitbook")
    )));
    let config = commitbook_engine::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.schedule, "*/15 * * * *");
}

#[tokio::test]
async fn config_update_reports_scheduler_failure_and_rolls_back() {
    let fake = Arc::new(FakeScheduler::running("0 * * * *"));
    fake.fail_next_install("launchctl load failed");
    let (tmp, app) = setup_test_app_with(Arc::clone(&fake));
    let (status, body) = post_json(
        app,
        "/api/config",
        serde_json::json!({"schedule": "*/15 * * * *"}),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("launchctl load failed"), "got: {body}");
    assert!(body.contains("were restored"), "got: {body}");
    assert_eq!(fake.installed_schedule().as_deref(), Some("0 * * * *"));
    let config = commitbook_engine::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.schedule, "0 * * * *");
}

#[tokio::test]
async fn config_update_rejected_while_repository_locked() {
    let (tmp, app) = setup_test_app();
    let _held = commitbook_engine::state::RepoLock::acquire(tmp.path()).unwrap();
    let (status, body) = post_json(
        app.clone(),
        "/api/config",
        serde_json::json!({"auto_push": false}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already running"), "got: {body}");
    let config = commitbook_engine::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert!(config.git.auto_push);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_api_config_partial_update() {
    let (tmp, app) = setup_test_app();
    let (status, _) = post_json(app, "/api/config", serde_json::json!({"auto_push": false})).await;
    assert_eq!(status, StatusCode::OK);

    let config = commitbook_engine::config::local::LocalConfig::load(tmp.path()).unwrap();
    assert!(!config.git.auto_push);
    assert_eq!(config.schedule, "0 * * * *"); // unchanged
}

#[tokio::test]
async fn test_api_providers_disabled_by_default() {
    let (_tmp, app) = setup_test_app();
    let (status, body): (_, Vec<ProviderInfo>) = get_json(app, "/api/providers").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_empty());
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

#[test]
fn test_escape_html_special_chars() {
    assert_eq!(
        escape_html("<script>alert('xss')</script>"),
        "&lt;script&gt;alert('xss')&lt;/script&gt;"
    );
    assert_eq!(escape_html("a&b"), "a&amp;b");
    assert_eq!(escape_html(r#"he said "hi""#), "he said &quot;hi&quot;");
    assert_eq!(escape_html("no special chars"), "no special chars");
}

#[tokio::test]
async fn test_api_config_error_is_escaped() {
    let (_tmp, app) = setup_test_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/config")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("schedule=%3Cscript%3Ealert(1)%3C%2Fscript%3E"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        body.contains("&lt;script&gt;"),
        "error should be HTML-escaped, got: {}",
        body
    );
    assert!(
        !body.contains("<script>"),
        "raw <script> should not appear in response"
    );
}

#[tokio::test]
async fn test_api_logs_filter_then_paginate() {
    let (tmp, app) = setup_test_app();

    let logs_dir = tmp.path().join(".CommitBook").join("local").join("logs");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_file = logs_dir.join(format!("{}.log", today));
    let mut entries = String::new();
    for i in 0..6 {
        let level = if i % 2 == 0 { "INFO" } else { "ERROR" };
        entries.push_str(&format!(
            r#"{{"ts":"2026-04-09 10:{:02}:00","level":"{}","msg":"entry {}"}}"#,
            i, level, i
        ));
        entries.push('\n');
    }
    std::fs::write(&log_file, entries).unwrap();

    // 3 ERROR entries total; request limit=2, should get exactly 2
    let (_, body): (_, LogsResponse) = get_json(app, "/api/logs?level=ERROR&limit=2").await;
    assert_eq!(body.entries.len(), 2);
    assert!(body.entries.iter().all(|e| e.level == "ERROR"));
    assert_eq!(body.total, 3); // total filtered count, not just page
}

#[tokio::test]
async fn test_dashboard_returns_html() {
    let (_tmp, app) = setup_test_app();
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

async fn get_html(app: Router, uri: &str) -> String {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn config_ai_messages_json_and_form_save_and_reload() {
    for content_type in ["application/json", "application/x-www-form-urlencoded"] {
        let (tmp, app) = setup_test_app();
        let initial = get_html(app.clone(), "/config").await;
        assert!(initial.contains("AI commit messages"));
        assert!(initial.contains(r#"value="false" selected>No"#));
        for enabled in [true, false] {
            let body = if content_type == "application/json" {
                serde_json::json!({"ai_messages": enabled, "auto_push": false}).to_string()
            } else {
                format!("ai_messages={enabled}&auto_push=false&schedule=0+*+*+*+*&branch=main")
            };
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/config")
                        .header("content-type", content_type)
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let response_body = response.into_body().collect().await.unwrap().to_bytes();
            assert!(String::from_utf8_lossy(&response_body).contains("Configuration saved."));
            let config = LocalConfig::load(tmp.path()).unwrap();
            assert_eq!(config.commit.ai_messages, enabled);
            assert!(!config.git.auto_push);
            let page = get_html(app.clone(), "/config").await;
            assert!(page.contains(&format!(
                r#"value="{enabled}" selected>{}"#,
                if enabled { "Yes" } else { "No" }
            )));
        }
    }
}

#[tokio::test]
async fn config_partial_updates_preserve_ai_opt_in() {
    let (tmp, app) = setup_test_app();
    post_json(
        app.clone(),
        "/api/config",
        serde_json::json!({"ai_messages": true}),
    )
    .await;
    post_json(
        app.clone(),
        "/api/config",
        serde_json::json!({"auto_push": false}),
    )
    .await;
    assert!(LocalConfig::load(tmp.path()).unwrap().commit.ai_messages);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/config")
                .header(
                    "content-type",
                    "application/x-www-form-urlencoded; charset=UTF-8",
                )
                .body(Body::from("auto_push=true"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let config = LocalConfig::load(tmp.path()).unwrap();
    assert!(config.commit.ai_messages);
    assert!(config.git.auto_push);
}

#[tokio::test]
async fn config_rejects_invalid_ai_values_without_saving() {
    for (content_type, body) in [
        ("application/json", r#"{"ai_messages":"yes"}"#),
        ("application/x-www-form-urlencoded", "ai_messages=yes"),
    ] {
        let (tmp, app) = setup_test_app();
        let before = std::fs::read(LocalConfig::config_path(tmp.path())).unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/config")
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error());
        assert_eq!(
            std::fs::read(LocalConfig::config_path(tmp.path())).unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn provider_status_disabled_on_default_or_invalid_config() {
    let (tmp, app) = setup_test_app();
    for invalid_config in [false, true] {
        if invalid_config {
            std::fs::write(LocalConfig::config_path(tmp.path()), "invalid = [").unwrap();
        }
        for uri in ["/", "/htmx/providers"] {
            let page = get_html(app.clone(), uri).await;
            assert!(page.contains("AI commit messages disabled"));
        }
        let (_, providers): (_, Vec<ProviderInfo>) = get_json(app.clone(), "/api/providers").await;
        assert!(providers.is_empty());
    }
}
