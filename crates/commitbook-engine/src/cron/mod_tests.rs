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
    assert!(parse_human_interval("0h").is_none());
    assert!(parse_human_interval("24h").is_none());
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
}

#[test]
fn test_cron_to_interval() {
    assert_eq!(cron_to_interval_seconds("*/5 * * * *"), 300);
    assert_eq!(cron_to_interval_seconds("*/30 * * * *"), 1800);
    assert_eq!(cron_to_interval_seconds("0 * * * *"), 3600);
    assert_eq!(cron_to_interval_seconds("0 */4 * * *"), 14400);
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
fn test_cron_to_interval_default_fallback() {
    // Non-standard expression should default to 3600
    assert_eq!(cron_to_interval_seconds("0 9 * * *"), 3600);
    // Malformed input defaults to 3600
    assert_eq!(cron_to_interval_seconds("not valid"), 3600);
}

#[test]
fn test_describe_schedule_dynamic() {
    assert_eq!(describe_schedule("*/7 * * * *"), "Every 7 minutes");
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
