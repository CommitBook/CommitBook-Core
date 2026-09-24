pub mod fake;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
#[allow(dead_code)]
pub mod linux;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
#[allow(dead_code)]
pub mod macos;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use fake::FakeScheduler;

/// Abstraction over the platform scheduler (launchd or crontab) so settings
/// operations and UI code can be exercised without touching the real one.
pub trait SchedulerAdapter: Send + Sync {
    /// Install or replace the job for `repo_root`.
    fn install(&self, repo_root: &Path, schedule: &str, binary: &Path) -> Result<()>;
    /// Remove the job for `repo_root`, if any.
    fn uninstall(&self, repo_root: &Path) -> Result<()>;
    /// Whether a job is currently loaded for `repo_root`.
    fn is_loaded(&self, repo_root: &Path) -> bool;
}

/// The real platform scheduler, delegating to the free functions below.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemScheduler;

impl SchedulerAdapter for SystemScheduler {
    fn install(&self, repo_root: &Path, schedule: &str, binary: &Path) -> Result<()> {
        install(repo_root, schedule, binary)
    }

    fn uninstall(&self, repo_root: &Path) -> Result<()> {
        uninstall(repo_root)
    }

    fn is_loaded(&self, repo_root: &Path) -> bool {
        is_loaded(repo_root)
    }
}

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

/// Convert a stored schedule (`1h`, `15m`, `daily`, a preset alias such as
/// `hourly`, or a cron expression) into a validated cron expression.
pub fn to_cron(schedule: &str) -> Result<String> {
    let trimmed = schedule.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Schedule must not be empty");
    }
    let preset = resolve_schedule(trimmed);
    if preset != trimmed {
        return Ok(preset);
    }
    if let Some(expression) = parse_human_interval(trimmed) {
        return Ok(expression);
    }
    validate_cron_expression(trimmed)?;
    Ok(trimmed.to_string())
}

/// The short form stored in `config.toml` for a cron expression that has
/// one (`*/15 * * * *` is `15m`, `0 9 * * *` is `daily`), else `None`.
pub fn short_form(cron_expr: &str) -> Option<String> {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    match parts.as_slice() {
        ["0", "9", "*", "*", "*"] => Some("daily".to_string()),
        ["0", "*", "*", "*", "*"] => Some("1h".to_string()),
        [minute, "*", "*", "*", "*"] => {
            let n: u32 = minute.strip_prefix("*/")?.parse().ok()?;
            (n > 0 && 60 % n == 0).then(|| format!("{n}m"))
        }
        ["0", hour, "*", "*", "*"] => {
            let n: u32 = hour.strip_prefix("*/")?.parse().ok()?;
            (n > 1 && 24 % n == 0).then(|| format!("{n}h"))
        }
        _ => None,
    }
}

/// Normalize user input for storage: validate it for this platform, then
/// keep the short form when one exists, otherwise the cron expression.
pub fn normalize_schedule(input: &str) -> Result<String> {
    let cron_expr = to_cron(input)?;
    validate_platform_schedule(&cron_expr)?;
    Ok(short_form(&cron_expr).unwrap_or(cron_expr))
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

    // `*/N` restarts at each clock-field boundary. It is only a uniform
    // "every N" interval when N divides that boundary exactly; for example,
    // `*/7` has a four-minute gap between 00:56 and 01:00.
    if let Some(step) = parts[0].strip_prefix("*/") {
        let n: u32 = step.parse().expect("step was validated above");
        if 60 % n != 0 {
            anyhow::bail!(
                "Minute interval {} does not divide evenly into 60; choose a divisor of 60",
                n
            );
        }
    }
    if let Some(step) = parts[1].strip_prefix("*/") {
        let n: u32 = step.parse().expect("step was validated above");
        if 24 % n != 0 {
            anyhow::bail!(
                "Hour interval {} does not divide evenly into 24; choose a divisor of 24",
                n
            );
        }
    }

    Ok(())
}

/// Validate both cron syntax and whether this platform's scheduler can
/// represent the schedule without changing its meaning.
pub fn validate_platform_schedule(schedule: &str) -> Result<()> {
    let expr = to_cron(schedule)?;
    let expr = expr.as_str();

    #[cfg(target_os = "macos")]
    {
        macos::validate_schedule(expr)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = expr;
        Ok(())
    }
}

/// Convert a cron expression to an interval in seconds (for launchd StartInterval).
pub fn cron_to_interval_seconds(schedule: &str) -> Result<u64> {
    let cron_expr = to_cron(schedule)?;
    let cron_expr = cron_expr.as_str();
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();

    if parts.as_slice() == ["*", "*", "*", "*", "*"] {
        return Ok(60);
    }

    let minute = parts[0];
    let hour = parts[1];

    // */N * * * * → every N minutes
    if parts[1..] == ["*", "*", "*", "*"] && minute.starts_with("*/") {
        let n: u64 = minute[2..].parse().expect("cron was validated above");
        return Ok(n * 60);
    }

    // 0 */N * * * → every N hours
    if minute == "0" && parts[2..] == ["*", "*", "*"] {
        if let Some(step) = hour.strip_prefix("*/") {
            let n: u64 = step.parse().expect("cron was validated above");
            return Ok(n * 3600);
        }
        if hour == "*" {
            return Ok(3600);
        }
    }

    anyhow::bail!(
        "Cron expression '{}' is calendar-based and cannot be represented as a launchd interval",
        cron_expr
    )
}

/// Human-readable description of a cron expression.
pub fn describe_schedule(schedule: &str) -> String {
    let cron_expr = to_cron(schedule).unwrap_or_else(|_| schedule.to_string());
    let cron_expr = cron_expr.as_str();
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
                if step
                    .parse::<u32>()
                    .is_ok_and(|minutes| minutes > 0 && 60 % minutes == 0)
                {
                    return format!("Every {} minutes", step);
                }
            }
            if parts[0] == "0" {
                if let Some(step) = parts[1].strip_prefix("*/") {
                    if step
                        .parse::<u32>()
                        .is_ok_and(|hours| hours > 0 && 24 % hours == 0)
                    {
                        return format!("Every {} hours", step);
                    }
                }
            }
            format!("Cron: {}", cron_expr)
        }
    }
}

/// Parse a natural-language interval like `5m`, `30min`, `1h`, `2hours`, `1d`
/// into an equivalent cron expression. Returns `None` for unsupported shapes
/// (sub-minute, > 23 hours, > 1 day) so the caller can fall through to the
/// strict cron validator with a clear error.
pub fn parse_human_interval(input: &str) -> Option<String> {
    let trimmed = input.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }

    // Split into leading digits and trailing unit. We require at least one
    // digit and a known unit suffix.
    let split_at = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)?;
    if split_at == 0 {
        return None;
    }
    let (n_str, unit) = trimmed.split_at(split_at);
    let n: u64 = n_str.parse().ok()?;
    let unit = unit.trim_start();

    match unit {
        "m" | "min" | "mins" | "minute" | "minutes" => {
            // A step is a uniform interval only when it divides the hour.
            if (1..=59).contains(&n) && 60 % n == 0 {
                Some(format!("*/{n} * * * *"))
            } else {
                None
            }
        }
        "h" | "hr" | "hrs" | "hour" | "hours" => {
            if n == 1 {
                Some("0 * * * *".to_string())
            } else if (2..=23).contains(&n) && 24 % n == 0 {
                Some(format!("0 */{n} * * *"))
            } else {
                None
            }
        }
        "d" | "day" | "days" => {
            if n == 1 {
                // Match the existing `daily` preset (9 AM).
                Some("0 9 * * *".to_string())
            } else {
                None
            }
        }
        _ => None,
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
pub fn install(repo_path: &Path, schedule: &str, commitbook_bin: &Path) -> Result<()> {
    let schedule = to_cron(schedule)?;
    let schedule = schedule.as_str();
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
pub fn uninstall(repo_path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::uninstall(repo_path)
    }

    #[cfg(target_os = "linux")]
    {
        linux::uninstall(repo_path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = repo_path;
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

/// Scheduler state as seen from the repository, beyond "is a job loaded".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum SchedulerHealth {
    Stopped,
    Running,
    /// A job is loaded but cannot run, e.g. its binary was deleted.
    Broken(String),
}

impl SchedulerHealth {
    pub fn is_loaded(&self) -> bool {
        !matches!(self, Self::Stopped)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Running => "running",
            Self::Broken(_) => "broken",
        }
    }

    /// Warning for a job that is loaded but not doing its work: a missing
    /// binary, or no sync attempt for three schedule intervals. Sleep can
    /// delay launchd runs briefly, so the stale threshold is generous.
    pub fn warning(
        &self,
        schedule: Option<&str>,
        last_attempt_at: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<String> {
        match self {
            Self::Stopped => None,
            Self::Broken(reason) => Some(format!(
                "Scheduler cannot run ({reason}). Run `commitbook doctor --fix` from an installed `commitbook` binary."
            )),
            Self::Running => {
                let schedule = schedule?;
                let last = DateTime::parse_from_rfc3339(last_attempt_at?).ok()?;
                let interval = cron_to_interval_seconds(schedule).unwrap_or(86_400);
                let threshold = chrono::Duration::seconds((interval * 3).max(1_800) as i64);
                (now.signed_duration_since(last) > threshold).then(|| {
                    format!(
                        "Scheduler has not run since {} (schedule: {}). Check `commitbook doctor`.",
                        last.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ"),
                        describe_schedule(schedule).to_lowercase()
                    )
                })
            }
        }
    }
}

/// Classify the repo's scheduler job: stopped, running, or loaded with a
/// binary that no longer exists (e.g. a deleted `target/` build).
pub fn health(repo_path: &Path) -> SchedulerHealth {
    if !is_loaded(repo_path) {
        return SchedulerHealth::Stopped;
    }
    match scheduled_binary(repo_path) {
        Some(binary) if !binary.exists() => {
            SchedulerHealth::Broken(format!("binary missing: {}", binary.display()))
        }
        _ => SchedulerHealth::Running,
    }
}

/// Binary path the repo's scheduler job launches, if one is installed.
pub fn scheduled_binary(repo_path: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        macos::scheduled_binary(repo_path)
    }

    #[cfg(target_os = "linux")]
    {
        linux::scheduled_binary(repo_path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = repo_path;
        None
    }
}

/// Whether `binary` looks like a build artifact that can disappear (a cargo
/// `target/` directory), making it a poor choice for a scheduler job.
pub fn is_transient_binary(binary: &Path) -> bool {
    let parts: Vec<_> = binary.components().map(|c| c.as_os_str()).collect();
    parts
        .windows(2)
        .any(|w| w[0] == "target" && (w[1] == "debug" || w[1] == "release"))
}

fn validate_cron_field(field: &str, max_val: u32) -> Result<()> {
    if field == "*" {
        return Ok(());
    }

    // */N
    if let Some(step) = field.strip_prefix("*/") {
        let n: u32 = step
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid step value"))?;
        if n < 1 || n > max_val {
            anyhow::bail!("step value {} out of range (1-{})", n, max_val);
        }
        return Ok(());
    }

    // N-M range
    if field.contains('-') {
        let parts: Vec<&str> = field.splitn(2, '-').collect();
        let low: u32 = parts[0]
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid range start"))?;
        let high: u32 = parts[1]
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid range end"))?;
        if low > max_val || high > max_val || low > high {
            anyhow::bail!("range {}-{} out of bounds (0-{})", low, high, max_val);
        }
        return Ok(());
    }

    // N,M,... list
    for part in field.split(',') {
        let n: u32 = part
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid value '{}'", part))?;
        if n > max_val {
            anyhow::bail!("value {} out of range (0-{})", n, max_val);
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
