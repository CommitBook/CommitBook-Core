use super::*;

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
