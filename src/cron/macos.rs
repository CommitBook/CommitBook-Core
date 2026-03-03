use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Generate a unique plist label from a repo path.
fn plist_label(repo_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_path.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("com.commitbook.{}", &hash[..12])
}

/// Get the path to the plist file.
fn plist_path(repo_path: &Path) -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    let label = plist_label(repo_path);
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", label))
}

/// Generate the plist XML content.
fn generate_plist(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
) -> String {
    let label = plist_label(repo_path);
    let repo_str = repo_path.to_string_lossy();
    let bin_str = commitbook_bin.to_string_lossy();

    // Parse the cron expression to get the interval in seconds
    let interval_seconds = cron_to_interval(schedule);

    let log_path = repo_path
        .join(".CommitBook")
        .join("logs")
        .join("launchd-stdout.log");
    let err_log_path = repo_path
        .join(".CommitBook")
        .join("logs")
        .join("launchd-stderr.log");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>auto-commit</string>
        <string>--repo</string>
        <string>{repo}</string>
    </array>
    <key>StartInterval</key>
    <integer>{interval}</integer>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>RunAtLoad</key>
    <false/>
</dict>
</plist>"#,
        label = label,
        bin = bin_str,
        repo = repo_str,
        interval = interval_seconds,
        stdout = log_path.to_string_lossy(),
        stderr = err_log_path.to_string_lossy(),
    )
}

/// Convert a simple cron expression to an interval in seconds.
/// This is a simplified conversion for common patterns.
fn cron_to_interval(cron_expr: &str) -> u64 {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    if parts.len() != 5 {
        return 3600; // default to 1 hour
    }

    let minute = parts[0];
    let hour = parts[1];

    // Check for */N minute patterns
    if let Some(step) = minute.strip_prefix("*/") {
        if let Ok(n) = step.parse::<u64>() {
            return n * 60;
        }
    }

    // Check for */N hour patterns
    if minute == "0" {
        if let Some(step) = hour.strip_prefix("*/") {
            if let Ok(n) = step.parse::<u64>() {
                return n * 3600;
            }
        }
        if hour == "*" {
            return 3600; // every hour
        }
    }

    // Default to daily (24 hours)
    86400
}

/// Install a launchd job for the repo.
pub fn install(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
) -> Result<String> {
    let path = plist_path(repo_path);
    let content = generate_plist(repo_path, schedule, commitbook_bin);

    // Ensure the LaunchAgents directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create LaunchAgents dir: {}", parent.display()))?;
    }

    // Unload existing job if present
    if path.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", &path.to_string_lossy()])
            .output();
    }

    // Write the plist
    fs::write(&path, content)
        .with_context(|| format!("Failed to write plist: {}", path.display()))?;

    // Load the job
    let output = Command::new("launchctl")
        .args(["load", &path.to_string_lossy()])
        .output()
        .context("Failed to run launchctl load")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("launchctl load failed: {}", stderr.trim());
    }

    Ok(path.to_string_lossy().to_string())
}

/// Uninstall a launchd job for the repo.
pub fn uninstall(repo_path: &Path, scheduler_id: Option<&str>) -> Result<()> {
    let path = if let Some(id) = scheduler_id {
        PathBuf::from(id)
    } else {
        plist_path(repo_path)
    };

    if path.exists() {
        // Unload the job
        let _ = Command::new("launchctl")
            .args(["unload", &path.to_string_lossy()])
            .output();

        // Remove the plist file
        fs::remove_file(&path)
            .with_context(|| format!("Failed to remove plist: {}", path.display()))?;
    }

    Ok(())
}

/// Check if launchd is accessible.
pub fn is_accessible() -> bool {
    Command::new("launchctl")
        .args(["list"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
