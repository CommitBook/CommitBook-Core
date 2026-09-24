use super::*;

#[test]
fn test_status_response_serialization() {
    let status = StatusResponse {
        repository: Default::default(),
        running: true,
        scheduler: commitbook_engine::cron::SchedulerHealth::Broken("binary missing: /gone".into()),
        scheduler_warning: Some("Scheduler cannot run".into()),
        schedule: "1h".into(),
        schedule_desc: "Every hour".into(),
        branch: "main".into(),
        current_branch: "main".into(),
        last_commit: Some("2026-04-07T14:30:02Z".into()),
        changes_total: Some(3),
        changes_summary: "2 modified, 1 new".into(),
    };
    let v = serde_json::to_value(&status).unwrap();
    assert_eq!(v["running"], true);
    assert_eq!(v["scheduler"]["state"], "broken");
    assert_eq!(v["scheduler"]["reason"], "binary missing: /gone");
    assert_eq!(v["scheduler_warning"], "Scheduler cannot run");
    assert_eq!(v["schedule"], "1h");
    assert_eq!(v["changes_total"], 3);
    assert!(v["last_commit"].is_string());
}

#[test]
fn test_log_entry_skips_none_provider() {
    let entry = LogEntry {
        timestamp: "2026-04-07 14:30:02".into(),
        level: "INFO".into(),
        message: "test".into(),
        provider: None,
    };
    let v = serde_json::to_value(&entry).unwrap();
    assert!(v.get("provider").is_none());
}

#[test]
fn test_log_entry_includes_provider() {
    let entry = LogEntry {
        timestamp: "2026-04-07 14:30:02".into(),
        level: "INFO".into(),
        message: "test".into(),
        provider: Some("Claude".into()),
    };
    let v = serde_json::to_value(&entry).unwrap();
    assert_eq!(v["provider"], "Claude");
}

#[test]
fn test_logs_query_defaults() {
    let query: LogsQuery = serde_json::from_str("{}").unwrap();
    assert!(query.limit.is_none());
    assert!(query.offset.is_none());
    assert!(query.level.is_none());
}

#[test]
fn test_logs_query_with_values() {
    let query: LogsQuery =
        serde_json::from_str(r#"{"limit": 10, "offset": 5, "level": "INFO"}"#).unwrap();
    assert_eq!(query.limit, Some(10));
    assert_eq!(query.offset, Some(5));
    assert_eq!(query.level.as_deref(), Some("INFO"));
}

#[test]
fn test_config_update_partial() {
    let update: ConfigUpdate = serde_json::from_str(r#"{"schedule": "5m"}"#).unwrap();
    assert_eq!(update.schedule.as_deref(), Some("5m"));
    assert!(update.conflict_mode.is_none());
    assert!(update.branch.is_none());
}

#[test]
fn test_action_response_serialization() {
    let resp = ActionResponse {
        success: true,
        message: "ok".into(),
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["message"], "ok");
}
