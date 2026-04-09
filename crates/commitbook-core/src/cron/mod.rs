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
#[path = "mod_tests.rs"]
mod tests;
