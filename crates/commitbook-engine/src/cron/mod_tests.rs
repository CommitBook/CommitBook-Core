use super::*;

#[test]
fn test_parse_human_interval_minutes() {
    assert_eq!(parse_human_interval("5m"), Some("*/5 * * * *".to_string()));
    assert_eq!(
        parse_human_interval("30m"),
        Some("*/30 * * * *".to_string())
    );
    assert_eq!(parse_human_interval("1m"), Some("*/1 * * * *".to_string()));
    assert_eq!(
        parse_human_interval("15min"),
        Some("*/15 * * * *".to_string())
    );
    assert_eq!(
        parse_human_interval("10minutes"),
        Some("*/10 * * * *".to_string())
    );
}

#[test]
fn test_parse_human_interval_hours() {
    assert_eq!(parse_human_interval("1h"), Some("0 * * * *".to_string()));
    assert_eq!(parse_human_interval("2h"), Some("0 */2 * * *".to_string()));
    assert_eq!(parse_human_interval("4hr"), Some("0 */4 * * *".to_string()));
    assert_eq!(
        parse_human_interval("12hours"),
        Some("0 */12 * * *".to_string())
    );
}

#[test]
fn test_parse_human_interval_days() {
    assert_eq!(parse_human_interval("1d"), Some("0 9 * * *".to_string()));
    assert_eq!(parse_human_interval("1day"), Some("0 9 * * *".to_string()));
}

#[test]
fn test_parse_human_interval_rejects_unsupported() {
    // Sub-minute intervals (launchd minimum is 1m via cron).
    assert!(parse_human_interval("30s").is_none());
    // Out-of-range minutes / hours.
    assert!(parse_human_interval("0m").is_none());
    assert!(parse_human_interval("60m").is_none());
    assert!(parse_human_interval("7m").is_none());
    assert!(parse_human_interval("0h").is_none());
    assert!(parse_human_interval("24h").is_none());
    assert!(parse_human_interval("5h").is_none());
    // Multi-day windows aren't expressible as a single launchd interval.
    assert!(parse_human_interval("2d").is_none());
    // Garbage input.
    assert!(parse_human_interval("").is_none());
    assert!(parse_human_interval("abc").is_none());
    assert!(parse_human_interval("5").is_none());
    assert!(parse_human_interval("m5").is_none());
}

#[test]
fn test_parse_human_interval_tolerates_whitespace_and_case() {
    assert_eq!(
        parse_human_interval("  5M "),
        Some("*/5 * * * *".to_string())
    );
    assert_eq!(parse_human_interval("1H"), Some("0 * * * *".to_string()));
}

#[test]
fn test_resolve_schedule_presets() {
    assert_eq!(resolve_schedule("hourly"), "0 * * * *");
    assert_eq!(resolve_schedule("daily"), "0 9 * * *");
    assert_eq!(resolve_schedule("every-4h"), "0 */4 * * *");
    assert_eq!(resolve_schedule("every-5m"), "*/5 * * * *");
}

#[test]
fn test_resolve_schedule_passthrough() {
    assert_eq!(resolve_schedule("*/5 * * * *"), "*/5 * * * *");
}

#[test]
fn test_validate_cron_valid() {
    assert!(validate_cron_expression("0 * * * *").is_ok());
    assert!(validate_cron_expression("*/15 * * * *").is_ok());
    assert!(validate_cron_expression("0 9 * * 1-5").is_ok());
}

#[test]
fn test_validate_cron_invalid() {
    assert!(validate_cron_expression("* *").is_err());
    assert!(validate_cron_expression("not a cron").is_err());
    assert!(validate_cron_expression("*/7 * * * *").is_err());
    assert!(validate_cron_expression("0 */5 * * *").is_err());
    assert!(validate_cron_expression("*/7 9 * * 1-5").is_err());
    assert!(validate_cron_expression("15 */5 * * 1-5").is_err());
}

#[test]
fn test_cron_to_interval() {
    assert_eq!(cron_to_interval_seconds("*/5 * * * *").unwrap(), 300);
    assert_eq!(cron_to_interval_seconds("*/30 * * * *").unwrap(), 1800);
    assert_eq!(cron_to_interval_seconds("0 * * * *").unwrap(), 3600);
    assert_eq!(cron_to_interval_seconds("0 */4 * * *").unwrap(), 14400);
}

#[test]
fn test_describe_schedule() {
    assert_eq!(describe_schedule("0 * * * *"), "Every hour");
    assert_eq!(describe_schedule("0 9 * * *"), "Daily at 9:00 AM");
    assert_eq!(describe_schedule("*/15 * * * *"), "Every 15 minutes");
}

#[test]
fn test_resolve_schedule_case_insensitive() {
    assert_eq!(resolve_schedule("Hourly"), "0 * * * *");
    assert_eq!(resolve_schedule("DAILY"), "0 9 * * *");
    assert_eq!(resolve_schedule("Every-5m"), "*/5 * * * *");
}

#[test]
fn test_resolve_schedule_all_aliases() {
    assert_eq!(resolve_schedule("every-5-min"), "*/5 * * * *");
    assert_eq!(resolve_schedule("every-15-min"), "*/15 * * * *");
    assert_eq!(resolve_schedule("every-30-min"), "*/30 * * * *");
    assert_eq!(resolve_schedule("every-1h"), "0 * * * *");
    assert_eq!(resolve_schedule("every-2-hours"), "0 */2 * * *");
    assert_eq!(resolve_schedule("every-4-hours"), "0 */4 * * *");
}

#[test]
fn test_validate_cron_invalid_minute_60() {
    assert!(validate_cron_expression("60 * * * *").is_err());
}

#[test]
fn test_validate_cron_invalid_hour_25() {
    assert!(validate_cron_expression("0 25 * * *").is_err());
}

#[test]
fn test_validate_cron_non_numeric() {
    assert!(validate_cron_expression("abc * * * *").is_err());
}

#[test]
fn test_validate_cron_empty() {
    assert!(validate_cron_expression("").is_err());
}

#[test]
fn test_validate_cron_range() {
    assert!(validate_cron_expression("0 9 * * 1-5").is_ok());
    assert!(validate_cron_expression("0 9 * * 8-9").is_err());
}

#[test]
fn test_cron_to_interval_rejects_non_interval_schedules() {
    assert!(cron_to_interval_seconds("0 9 * * *").is_err());
    assert!(cron_to_interval_seconds("not valid").is_err());
}

#[test]
fn test_describe_schedule_dynamic() {
    assert_eq!(describe_schedule("*/7 * * * *"), "Cron: */7 * * * *");
    assert_eq!(describe_schedule("0 */3 * * *"), "Every 3 hours");
    assert_eq!(describe_schedule("bad"), "bad");
}

#[test]
fn test_list_presets_content() {
    let presets = list_presets();
    assert!(presets.contains("every-5m"));
    assert!(presets.contains("hourly"));
    assert!(presets.contains("daily"));
    assert!(presets.contains("every-2h"));
}

fn at(ts: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

#[test]
fn test_health_warning_stopped_is_silent() {
    let warning = SchedulerHealth::Stopped.warning(
        Some("0 * * * *"),
        Some("2026-09-09T11:02:31Z"),
        at("2026-09-24T19:00:00Z"),
    );
    assert_eq!(warning, None);
}

#[test]
fn test_health_warning_broken_names_reason_and_fix() {
    let warning = SchedulerHealth::Broken("binary missing: /gone/commitbook".into())
        .warning(None, None, at("2026-09-24T19:00:00Z"))
        .unwrap();
    assert!(warning.contains("binary missing: /gone/commitbook"));
    assert!(warning.contains("commitbook doctor --fix"));
}

#[test]
fn test_health_warning_running_but_stale() {
    let warning = SchedulerHealth::Running
        .warning(
            Some("0 * * * *"),
            Some("2026-09-09T11:02:31Z"),
            at("2026-09-24T19:00:00Z"),
        )
        .unwrap();
    assert!(warning.contains("has not run since 2026-09-09T11:02:31Z"));
    assert!(warning.contains("every hour"));
}

#[test]
fn test_health_warning_running_recently_is_silent() {
    // Hourly: three intervals is the threshold.
    let now = at("2026-09-24T19:00:00Z");
    let fresh =
        SchedulerHealth::Running.warning(Some("0 * * * *"), Some("2026-09-24T16:30:00Z"), now);
    assert_eq!(fresh, None);
    let stale =
        SchedulerHealth::Running.warning(Some("0 * * * *"), Some("2026-09-24T15:30:00Z"), now);
    assert!(stale.is_some());
}

#[test]
fn test_health_warning_short_interval_uses_minimum_threshold() {
    // Every 5 minutes would be 15 minutes; the floor is 30 minutes.
    let now = at("2026-09-24T19:00:00Z");
    let within =
        SchedulerHealth::Running.warning(Some("*/5 * * * *"), Some("2026-09-24T18:40:00Z"), now);
    assert_eq!(within, None);
}

#[test]
fn test_health_warning_needs_schedule_and_attempt() {
    let now = at("2026-09-24T19:00:00Z");
    assert_eq!(
        SchedulerHealth::Running.warning(None, Some("2026-09-01T00:00:00Z"), now),
        None
    );
    assert_eq!(
        SchedulerHealth::Running.warning(Some("0 * * * *"), None, now),
        None
    );
    assert_eq!(
        SchedulerHealth::Running.warning(Some("0 * * * *"), Some("garbage"), now),
        None
    );
}

#[test]
fn test_health_serializes_with_state_and_reason() {
    let broken = serde_json::to_value(SchedulerHealth::Broken("x".into())).unwrap();
    assert_eq!(
        broken,
        serde_json::json!({"state": "broken", "reason": "x"})
    );
    let running = serde_json::to_value(SchedulerHealth::Running).unwrap();
    assert_eq!(running, serde_json::json!({"state": "running"}));
}

#[test]
fn test_is_transient_binary() {
    assert!(is_transient_binary(Path::new(
        "/Users/m/workspaces/rome/target/debug/commitbook"
    )));
    assert!(is_transient_binary(Path::new(
        "/src/cb/target/release/commitbook"
    )));
    assert!(!is_transient_binary(Path::new(
        "/Users/m/.cargo/bin/commitbook"
    )));
    assert!(!is_transient_binary(Path::new(
        "/Users/m/.local/bin/commitbook"
    )));
    assert!(!is_transient_binary(Path::new("/opt/target/commitbook")));
}
