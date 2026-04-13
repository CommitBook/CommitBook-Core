use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::{clean_message, truncate, CommitMessageProvider};
use crate::git::ChangesSummary;

const COPILOT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct CopilotProvider;

#[async_trait]
impl CommitMessageProvider for CopilotProvider {
    fn name(&self) -> &str {
        "GitHub Copilot"
    }

    fn key(&self) -> &str {
        "gh-copilot"
    }

    fn is_available(&self) -> bool {
        if which::which("gh").is_err() {
            return false;
        }
        Command::new("gh")
            .args(["copilot", "-v"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn generate(&self, summary: &ChangesSummary, repo_path: &Path) -> Result<String> {
        let diff_text = get_diff_summary(repo_path)?;
        let prompt = format!(
            "Write a concise one-line git commit message (max 72 chars) for these changes: {}. Diff:\n{}",
            summary.to_summary_text(),
            truncate(&diff_text, 500)
        );

        let child = Command::new("gh")
            .args(["copilot", "-p", &prompt, "--no-color"])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to run gh copilot suggest")?;

        let output = wait_with_timeout(child, COPILOT_TIMEOUT)
            .context("gh copilot suggest timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gh copilot suggest failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = extract_message(&stdout);

        if message.is_empty() {
            bail!("GitHub Copilot returned empty response");
        }

        Ok(message)
    }
}

/// Check if gh CLI is authenticated.
pub fn is_authenticated() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Parse the output from gh copilot to extract the commit message.
///
/// The new `gh copilot -p` output may contain:
/// - Tool invocation lines (✓, $, ↪)
/// - Usage statistics (Total usage est:, Total duration:, Usage by model:)
/// - The commit message as plain text or inside a fenced code block
fn extract_message(output: &str) -> String {
    // First, try to extract from a fenced code block (```...```)
    if let Some(msg) = extract_from_code_block(output) {
        return clean_message(&msg);
    }

    // Otherwise, filter out noise and find the commit message
    let lines: Vec<&str> = output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    for line in lines.iter().rev() {
        let cleaned = line.trim_start_matches(['>', '#', '-', '*', ' ']);
        if cleaned.is_empty() {
            continue;
        }
        // Skip usage statistics from new copilot CLI
        if is_copilot_noise(cleaned) {
            continue;
        }
        // Skip meta-lines from copilot output
        if cleaned.starts_with("Suggestion")
            || cleaned.starts_with('?')
            || cleaned.to_lowercase().contains("copilot")
        {
            continue;
        }
        // Strip "git commit -m " prefix if present
        let cleaned = cleaned
            .strip_prefix("git commit -m ")
            .unwrap_or(cleaned);
        let cleaned = cleaned.trim_matches('"').trim_matches('\'');
        if !cleaned.is_empty() {
            return clean_message(cleaned);
        }
    }

    if let Some(last) = lines.last() {
        if !is_copilot_noise(last) {
            return clean_message(last);
        }
    }

    String::new()
}

/// Extract commit message from a fenced code block in the output.
fn extract_from_code_block(output: &str) -> Option<String> {
    let mut in_block = false;
    let mut content = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_block {
                // End of block — return what we collected
                let msg = content.join("\n").trim().to_string();
                if !msg.is_empty() {
                    return Some(msg);
                }
                return None;
            }
            in_block = true;
            continue;
        }
        if in_block {
            content.push(trimmed);
        }
    }
    None
}

/// Returns true if the line is copilot CLI noise (tool output, stats, etc.)
fn is_copilot_noise(line: &str) -> bool {
    // Tool invocation lines
    if line.starts_with("✓") || line.starts_with("✗") {
        return true;
    }
    if line.starts_with("$ ") || line.starts_with("↪") {
        return true;
    }
    // Usage statistics
    if line.starts_with("Total usage")
        || line.starts_with("Total duration")
        || line.starts_with("Total code changes")
        || line.starts_with("Usage by model")
    {
        return true;
    }
    // Model usage lines (e.g. "    claude-sonnet-4.5    26.3k input, ...")
    if line.contains("input,") && line.contains("output,") && line.contains("cache") {
        return true;
    }
    // Label lines like "**Commit message:**"
    if line.contains("Commit message") && line.contains("**") {
        return true;
    }
    false
}

/// Get the staged or working-directory diff summary for prompt context.
fn get_diff_summary(repo_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "--staged", "--stat"])
        .current_dir(repo_path)
        .output()
        .context("Failed to get staged diff summary")?;

    if output.status.success() && !output.stdout.is_empty() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let output = Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(repo_path)
        .output()
        .context("Failed to get diff summary")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Wait for a child process with a timeout.
fn wait_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    let mut child = child;
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .context("Failed to get process output");
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    bail!("Process timed out after {:?}", timeout);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => bail!("Error waiting for process: {}", e),
        }
    }
}

#[cfg(test)]
#[path = "copilot_tests.rs"]
mod tests;
