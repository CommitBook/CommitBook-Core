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
    format!("com.zaai.commitbook.{}", &hash[..12])
}

/// Get the path where the plist file should be written.
pub fn plist_path(repo_path: &Path) -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    let label = plist_label(repo_path);
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", label))
}

/// Binaries commitbook's scheduled runs invoke. Their parent directories
/// are resolved at install time and written into the plist's PATH so the
/// captured PATH is narrow instead of inheriting the full shell PATH
/// (which can reach into TCC-protected roots like ~/Downloads and cause
/// macOS to prompt for folder access on every scheduled run).
const REQUIRED_TOOLS: &[&str] = &["git", "claude", "codex", "gh", "gemini", "cursor-agent"];

/// Baseline directories always included so scheduled runs keep working
/// if a tool is installed into a standard location after the scheduler
/// was registered. `launchctl` lives in `/bin`, already covered here.
const BASELINE_PATHS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
];

/// TCC-protected roots (relative to `$HOME`). Any PATH entry that sits
/// under one of these triggers macOS's Files-and-Folders prompt when the
/// scheduled process performs PATH lookups, so we strip them.
const PROTECTED_SUBPATHS: &[&str] = &[
    "Downloads",
    "Desktop",
    "Documents",
    "Library/Mobile Documents",
];

/// Resolve a binary's parent directory via `which`.
fn which_parent(tool: &str) -> Option<String> {
    let output = Command::new("/usr/bin/which").arg(tool).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().find(|l| !l.trim().is_empty())?.trim();
    Path::new(first)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
}

/// Remove PATH entries that live under a TCC-protected root.
fn filter_protected(entries: Vec<String>, home: &Path) -> Vec<String> {
    let protected: Vec<PathBuf> = PROTECTED_SUBPATHS
        .iter()
        .map(|sub| home.join(sub))
        .collect();
    entries
        .into_iter()
        .filter(|entry| {
            let entry_path = Path::new(entry);
            !protected.iter().any(|root| entry_path.starts_with(root))
        })
        .collect()
}

/// Build the minimal PATH string written into the generated plist.
fn build_plist_path() -> String {
    let mut entries: Vec<String> = Vec::new();

    for tool in REQUIRED_TOOLS {
        if let Some(dir) = which_parent(tool) {
            entries.push(dir);
        }
    }

    for base in BASELINE_PATHS {
        entries.push((*base).to_string());
    }

    if let Some(home) = dirs::home_dir() {
        entries.push(home.join(".cargo").join("bin").to_string_lossy().to_string());
    }

    let home_for_filter = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let filtered = filter_protected(entries, &home_for_filter);

    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<String> = filtered
        .into_iter()
        .filter(|e| seen.insert(e.clone()))
        .collect();

    deduped.join(":")
}

/// Escape special XML characters in a string value.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
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

    let logs_dir = repo_path.join(".CommitBook").join("local").join("logs");
    let stdout_log = logs_dir.join("launchd-stdout.log");
    let stderr_log = logs_dir.join("launchd-stderr.log");

    // Build a minimal PATH from resolved tool locations + baseline dirs.
    // Avoids leaking TCC-protected roots (e.g. ~/Downloads) from the shell PATH.
    let path_env = build_plist_path();

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
        <string>run</string>
    </array>
    <key>WorkingDirectory</key>
    <string>{repo}</string>
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
        bin = xml_escape(&bin_str),
        repo = xml_escape(&repo_str),
        interval = interval,
        stdout = xml_escape(&stdout_log.to_string_lossy()),
        stderr = xml_escape(&stderr_log.to_string_lossy()),
        path = xml_escape(&path_env),
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
