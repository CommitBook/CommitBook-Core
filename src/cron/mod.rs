#[allow(dead_code)]
pub mod macos;
#[allow(dead_code)]
pub mod linux;

use anyhow::Result;
use std::path::Path;

/// Cron schedule presets.
pub fn resolve_schedule(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "hourly" => "0 * * * *".to_string(),
        "daily" => "0 9 * * *".to_string(),
        "every-4h" | "every-4-hours" => "0 */4 * * *".to_string(),
        "every-2h" | "every-2-hours" => "0 */2 * * *".to_string(),
        "every-30m" | "every-30-min" => "*/30 * * * *".to_string(),
        "every-15m" | "every-15-min" => "*/15 * * * *".to_string(),
        _ => input.to_string(), // Assume raw cron expression
    }
}

/// Validate that a string looks like a valid cron expression.
pub fn validate_cron_expression(expr: &str) -> Result<()> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        anyhow::bail!(
            "Invalid cron expression '{}': expected 5 fields (minute hour day month weekday), got {}",
            expr,
            parts.len()
        );
    }

    let labels = ["minute", "hour", "day", "month", "weekday"];
    let max_values = [59, 23, 31, 12, 6];

    for (i, (part, label)) in parts.iter().zip(labels.iter()).enumerate() {
        if !is_valid_cron_field(part, max_values[i]) {
            anyhow::bail!(
                "Invalid cron field for {}: '{}'. Expected number, *, */N, or range.",
                label,
                part
            );
        }
    }

    Ok(())
}

/// Check if a single cron field is syntactically valid.
fn is_valid_cron_field(field: &str, max_val: u32) -> bool {
    if field == "*" {
        return true;
    }

    // Handle */N (step values)
    if let Some(step) = field.strip_prefix("*/") {
        return step.parse::<u32>().map(|n| n > 0 && n <= max_val).unwrap_or(false);
    }

    // Handle ranges like 1-5
    if field.contains('-') {
        let parts: Vec<&str> = field.split('-').collect();
        if parts.len() != 2 {
            return false;
        }
        let start = parts[0].parse::<u32>();
        let end = parts[1].parse::<u32>();
        return start.is_ok() && end.is_ok();
    }

    // Handle comma-separated values like 1,3,5
    if field.contains(',') {
        return field.split(',').all(|v| v.parse::<u32>().is_ok());
    }

    // Plain number
    field.parse::<u32>().map(|n| n <= max_val).unwrap_or(false)
}

/// Install a cron/launchd job for a repo. Platform-specific.
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
        anyhow::bail!("Unsupported operating system for cron scheduling")
    }
}

/// Remove a cron/launchd job for a repo. Platform-specific.
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
        anyhow::bail!("Unsupported operating system for cron scheduling")
    }
}

/// Check if the scheduling system is accessible.
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

/// Get a human-readable description of a cron schedule.
pub fn describe_schedule(cron_expr: &str) -> String {
    match cron_expr {
        "0 * * * *" => "Every hour".to_string(),
        "0 9 * * *" => "Daily at 9:00 AM".to_string(),
        "0 */4 * * *" => "Every 4 hours".to_string(),
        "0 */2 * * *" => "Every 2 hours".to_string(),
        "*/30 * * * *" => "Every 30 minutes".to_string(),
        "*/15 * * * *" => "Every 15 minutes".to_string(),
        _ => format!("Cron: {}", cron_expr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_schedule_presets() {
        assert_eq!(resolve_schedule("hourly"), "0 * * * *");
        assert_eq!(resolve_schedule("daily"), "0 9 * * *");
        assert_eq!(resolve_schedule("every-4h"), "0 */4 * * *");
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
}
