#[allow(dead_code)]
pub mod macos;
#[allow(dead_code)]
pub mod linux;

use anyhow::Result;
use std::path::Path;

/// Cron schedule presets mapped to 5-field cron expressions.
pub fn resolve_schedule(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "every-5m" | "every-5-min" => "*/5 * * * *".to_string(),
        "every-15m" | "every-15-min" => "*/15 * * * *".to_string(),
        "every-30m" | "every-30-min" => "*/30 * * * *".to_string(),
        "hourly" | "every-1h" => "0 * * * *".to_string(),
        "every-2h" | "every-2-hours" => "0 */2 * * *".to_string(),
        "every-4h" | "every-4-hours" => "0 */4 * * *".to_string(),
        "daily" => "0 9 * * *".to_string(),
        _ => input.to_string(),
    }
}

/// Validate a 5-field cron expression.
pub fn validate_cron_expression(expr: &str) -> Result<()> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        anyhow::bail!(
            "Invalid cron expression '{}': expected 5 fields, got {}",
            expr,
            parts.len()
        );
    }

    let labels = ["minute", "hour", "day-of-month", "month", "day-of-week"];
    let max_values = [59, 23, 31, 12, 7];

    for (i, (part, label)) in parts.iter().zip(labels.iter()).enumerate() {
        validate_cron_field(part, max_values[i])
            .map_err(|e| anyhow::anyhow!("Invalid {} field '{}': {}", label, part, e))?;
    }

    Ok(())
}

/// Convert a cron expression to an interval in seconds (for launchd StartInterval).
pub fn cron_to_interval_seconds(cron_expr: &str) -> u64 {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    if parts.len() != 5 {
        return 3600;
    }

    let minute = parts[0];
    let hour = parts[1];

    // */N * * * * → every N minutes
    if let Some(step) = minute.strip_prefix("*/") {
        if let Ok(n) = step.parse::<u64>() {
            if n > 0 {
                return n * 60;
            }
        }
    }

    // 0 */N * * * → every N hours
    if minute == "0" {
        if let Some(step) = hour.strip_prefix("*/") {
            if let Ok(n) = step.parse::<u64>() {
                if n > 0 {
                    return n * 3600;
                }
            }
        }
        if hour == "*" {
            return 3600;
        }
    }

    3600 // default: hourly
}

/// Human-readable description of a cron expression.
pub fn describe_schedule(cron_expr: &str) -> String {
    match cron_expr {
        "*/5 * * * *" => "Every 5 minutes".to_string(),
        "*/15 * * * *" => "Every 15 minutes".to_string(),
        "*/30 * * * *" => "Every 30 minutes".to_string(),
        "0 * * * *" => "Every hour".to_string(),
        "0 */2 * * *" => "Every 2 hours".to_string(),
        "0 */4 * * *" => "Every 4 hours".to_string(),
        "0 9 * * *" => "Daily at 9:00 AM".to_string(),
        _ => {
            let parts: Vec<&str> = cron_expr.split_whitespace().collect();
            if parts.len() != 5 {
                return cron_expr.to_string();
            }
            if let Some(step) = parts[0].strip_prefix("*/") {
                return format!("Every {} minutes", step);
            }
            if parts[0] == "0" {
                if let Some(step) = parts[1].strip_prefix("*/") {
                    return format!("Every {} hours", step);
                }
            }
            format!("Cron: {}", cron_expr)
        }
    }
}

/// List available schedule presets as formatted text.
pub fn list_presets() -> &'static str {
    "Available presets:
  every-5m   - Every 5 minutes
  every-15m  - Every 15 minutes
  every-30m  - Every 30 minutes
  hourly     - Every hour (default)
  every-2h   - Every 2 hours
  every-4h   - Every 4 hours
  daily      - Daily at 9:00 AM"
}

/// Install a scheduler job for a repo. Platform-specific.
pub fn install(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        macos::install(repo_path, schedule, commitbook_bin)
    }

    #[cfg(target_os = "linux")]
    {
        linux::install(repo_path, schedule, commitbook_bin)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (repo_path, schedule, commitbook_bin);
        anyhow::bail!("Unsupported operating system for scheduling")
    }
}

/// Remove a scheduler job for a repo. Platform-specific.
pub fn uninstall(repo_path: &Path, scheduler_id: Option<&str>) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::uninstall(repo_path, scheduler_id)
    }

    #[cfg(target_os = "linux")]
    {
        linux::uninstall(repo_path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (repo_path, scheduler_id);
        anyhow::bail!("Unsupported operating system for scheduling")
    }
}

/// Check if the scheduling system is accessible on this platform.
pub fn is_accessible() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_accessible()
    }

    #[cfg(target_os = "linux")]
    {
        linux::is_accessible()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// Check if a launchd/cron job is currently loaded for the given repo.
pub fn is_loaded(repo_path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_loaded(repo_path)
    }

    #[cfg(target_os = "linux")]
    {
        linux::is_loaded(repo_path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = repo_path;
        false
    }
}

fn validate_cron_field(field: &str, max_val: u32) -> Result<()> {
    if field == "*" {
        return Ok(());
    }

    // */N
    if let Some(step) = field.strip_prefix("*/") {
        let n: u32 = step.parse().map_err(|_| anyhow::anyhow!("invalid step value"))?;
        if n < 1 || n > max_val {
            anyhow::bail!("step value {} out of range (1-{})", n, max_val);
        }
        return Ok(());
    }

    // N-M range
    if field.contains('-') {
        let parts: Vec<&str> = field.splitn(2, '-').collect();
        let low: u32 = parts[0].parse().map_err(|_| anyhow::anyhow!("invalid range start"))?;
        let high: u32 = parts[1].parse().map_err(|_| anyhow::anyhow!("invalid range end"))?;
        if low > max_val || high > max_val || low > high {
            anyhow::bail!("range {}-{} out of bounds (0-{})", low, high, max_val);
        }
        return Ok(());
    }

    // N,M,... list
    for part in field.split(',') {
        let n: u32 = part.trim().parse().map_err(|_| anyhow::anyhow!("invalid value '{}'", part))?;
        if n > max_val {
            anyhow::bail!("value {} out of range (0-{})", n, max_val);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
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

    // --- Phase 2a: 10 new tests ---

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
}
