use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::cron_to_interval_seconds;

/// Generate a unique plist label from a repo path.
pub fn plist_label(repo_path: &Path) -> String {
    let canonical = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("com.commitbook.{}", &hash[..12])
}

/// Get the path where the plist file should be written.
pub fn plist_path(repo_path: &Path) -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    let label = plist_label(repo_path);
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", label))
}

/// Generate the plist XML content with PATH environment variable baked in.
fn generate_plist(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
) -> String {
    let label = plist_label(repo_path);
    let repo_str = repo_path.to_string_lossy();
    let bin_str = commitbook_bin.to_string_lossy();
    let interval = cron_to_interval_seconds(schedule);

    let logs_dir = repo_path.join(".CommitBook").join("logs");
    let stdout_log = logs_dir.join("launchd-stdout.log");
    let stderr_log = logs_dir.join("launchd-stderr.log");

    // Capture current PATH so AI CLIs (claude, codex, gh) are discoverable
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string());

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
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{path}</string>
    </dict>
</dict>
</plist>"#,
        label = label,
        bin = bin_str,
        repo = repo_str,
        interval = interval,
        stdout = stdout_log.to_string_lossy(),
        stderr = stderr_log.to_string_lossy(),
        path = path_env,
    )
}

/// Install a launchd job for the repo. Returns the plist path as scheduler_id.
pub fn install(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
) -> Result<String> {
    let path = plist_path(repo_path);
    let content = generate_plist(repo_path, schedule, commitbook_bin);

    // Ensure LaunchAgents directory exists
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
        let _ = Command::new("launchctl")
            .args(["unload", &path.to_string_lossy()])
            .output();

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

/// Check if a launchd job is currently loaded for the given repo.
pub fn is_loaded(repo_path: &Path) -> bool {
    let label = plist_label(repo_path);
    Command::new("launchctl")
        .args(["list", &label])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Validate that the binary path in the plist still exists.
#[allow(dead_code)]
pub fn validate_binary_path(repo_path: &Path) -> Result<bool> {
    let path = plist_path(repo_path);
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| "Failed to read plist")?;

    // Extract binary path from ProgramArguments (first <string> after the array)
    if let Some(start) = content.find("<array>") {
        if let Some(str_start) = content[start..].find("<string>") {
            let offset = start + str_start + 8;
            if let Some(str_end) = content[offset..].find("</string>") {
                let bin_path = &content[offset..offset + str_end];
                return Ok(Path::new(bin_path).exists());
            }
        }
    }

    Ok(false)
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;
