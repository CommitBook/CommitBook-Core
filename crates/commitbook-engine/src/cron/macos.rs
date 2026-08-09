use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{cron_to_interval_seconds, validate_cron_expression};

const PLIST_LABEL_PREFIX: &str = "com.zaai.commitbook";
const LEGACY_PLIST_LABEL_PREFIX: &str = "com.commitbook";

fn repository_hash(repo_path: &Path) -> String {
    let canonical = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    hash[..12].to_string()
}

/// Generate a unique plist label from a repo path.
pub fn plist_label(repo_path: &Path) -> String {
    format!("{PLIST_LABEL_PREFIX}.{}", repository_hash(repo_path))
}

/// Generate the label used before the com.zaai.commitbook rename.
pub fn legacy_plist_label(repo_path: &Path) -> String {
    format!("{LEGACY_PLIST_LABEL_PREFIX}.{}", repository_hash(repo_path))
}

/// Get the path where the plist file should be written.
pub fn plist_path(repo_path: &Path) -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    plist_path_for_label(&home, &plist_label(repo_path))
}

/// Get the path used by releases before the launchd label rename.
pub fn legacy_plist_path(repo_path: &Path) -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    plist_path_for_label(&home, &legacy_plist_label(repo_path))
}

fn plist_path_for_label(home: &Path, label: &str) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", label))
}

/// Return an installed current or legacy plist, preferring the current one.
pub fn existing_plist_path(repo_path: &Path) -> Option<PathBuf> {
    [plist_path(repo_path), legacy_plist_path(repo_path)]
        .into_iter()
        .find(|path| path.exists())
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
const BASELINE_PATHS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

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
        entries.push(
            home.join(".cargo")
                .join("bin")
                .to_string_lossy()
                .to_string(),
        );
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

fn launchd_schedule_xml(schedule: &str) -> Result<String> {
    if let Ok(interval) = cron_to_interval_seconds(schedule) {
        return Ok(format!(
            "    <key>StartInterval</key>\n    <integer>{interval}</integer>"
        ));
    }

    validate_cron_expression(schedule)?;
    let parts: Vec<&str> = schedule.split_whitespace().collect();
    if parts[2..] != ["*", "*", "*"] {
        anyhow::bail!(
            "Cron expression '{}' cannot be translated safely to launchd; use an every-N interval or a daily hour/minute schedule",
            schedule
        );
    }

    let minute: u32 = parts[0].parse().map_err(|_| {
        anyhow::anyhow!(
            "Cron expression '{}' cannot be translated safely to launchd; minute must be fixed",
            schedule
        )
    })?;
    let mut fields =
        format!("            <key>Minute</key>\n            <integer>{minute}</integer>");
    if parts[1] != "*" {
        let hour: u32 = parts[1].parse().map_err(|_| {
            anyhow::anyhow!(
                "Cron expression '{}' cannot be translated safely to launchd; hour must be fixed",
                schedule
            )
        })?;
        fields.push_str(&format!(
            "\n            <key>Hour</key>\n            <integer>{hour}</integer>"
        ));
    }

    Ok(format!(
        "    <key>StartCalendarInterval</key>\n    <dict>\n{fields}\n    </dict>"
    ))
}

pub(super) fn validate_schedule(schedule: &str) -> Result<()> {
    launchd_schedule_xml(schedule).map(|_| ())
}

/// Generate the plist XML content with PATH environment variable baked in.
fn generate_plist(repo_path: &Path, schedule: &str, commitbook_bin: &Path) -> Result<String> {
    let label = plist_label(repo_path);
    let repo_str = repo_path.to_string_lossy();
    let bin_str = commitbook_bin.to_string_lossy();
    let schedule_xml = launchd_schedule_xml(schedule)?;

    let logs_dir = repo_path.join(".CommitBook").join("local").join("logs");
    let stdout_log = logs_dir.join("launchd-stdout.log");
    let stderr_log = logs_dir.join("launchd-stderr.log");

    // Build a minimal PATH from resolved tool locations + baseline dirs.
    // Avoids leaking TCC-protected roots (e.g. ~/Downloads) from the shell PATH.
    let path_env = build_plist_path();

    Ok(format!(
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
{schedule_xml}
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
        schedule_xml = schedule_xml,
        stdout = xml_escape(&stdout_log.to_string_lossy()),
        stderr = xml_escape(&stderr_log.to_string_lossy()),
        path = xml_escape(&path_env),
    ))
}

fn label_is_loaded(label: &str) -> bool {
    Command::new("launchctl")
        .args(["list", label])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn unload_and_remove(path: &Path, label: &str) -> Result<()> {
    if label_is_loaded(label) {
        let output = Command::new("launchctl")
            .args(["remove", label])
            .output()
            .with_context(|| format!("Failed to unload launchd job {label}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to unload launchd job {}: {}", label, stderr.trim());
        }
    }

    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("Failed to remove plist: {}", path.display()))?;
    }
    Ok(())
}

/// Install a launchd job for the repo. Returns the plist path as scheduler_id.
pub fn install(repo_path: &Path, schedule: &str, commitbook_bin: &Path) -> Result<String> {
    let path = plist_path(repo_path);
    let legacy_path = legacy_plist_path(repo_path);
    // Render first so a bad schedule cannot remove a currently working job.
    let content = generate_plist(repo_path, schedule, commitbook_bin)?;

    // Ensure LaunchAgents directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create LaunchAgents dir: {}", parent.display()))?;
    }

    // Leave exactly one current job, migrating the previous label if needed.
    unload_and_remove(&path, &plist_label(repo_path))?;
    unload_and_remove(&legacy_path, &legacy_plist_label(repo_path))?;

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
    let _ = scheduler_id;
    unload_and_remove(&plist_path(repo_path), &plist_label(repo_path))?;
    unload_and_remove(
        &legacy_plist_path(repo_path),
        &legacy_plist_label(repo_path),
    )
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
    [plist_label(repo_path), legacy_plist_label(repo_path)]
        .iter()
        .any(|label| label_is_loaded(label))
}

/// Check specifically for a loaded job using the pre-rename label.
pub fn is_legacy_loaded(repo_path: &Path) -> bool {
    label_is_loaded(&legacy_plist_label(repo_path))
}

/// Validate that the binary path in the plist still exists.
#[allow(dead_code)]
pub fn validate_binary_path(repo_path: &Path) -> Result<bool> {
    let Some(path) = existing_plist_path(repo_path) else {
        return Ok(false);
    };

    let content = fs::read_to_string(&path).with_context(|| "Failed to read plist")?;

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
