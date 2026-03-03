use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::git::operations::ChangesSummary;

/// Check if GitHub Copilot CLI is available.
pub fn is_available() -> bool {
    // Check if gh CLI exists
    if which::which("gh").is_err() {
        return false;
    }

    // Check if copilot extension is installed
    let output = Command::new("gh")
        .args(["copilot", "--version"])
        .output();

    matches!(output, Ok(o) if o.status.success())
}

/// Check if gh CLI is authenticated.
pub fn is_authenticated() -> bool {
    let output = Command::new("gh")
        .args(["auth", "status"])
        .output();

    matches!(output, Ok(o) if o.status.success())
}

/// Generate a commit message using GitHub Copilot CLI.
pub async fn generate(summary: &ChangesSummary, repo_path: &Path) -> Result<String> {
    if !is_available() {
        bail!("GitHub Copilot CLI is not available");
    }

    let diff_text = get_diff_summary(repo_path)?;
    let prompt = format!(
        "Write a concise git commit message (one line, max 72 chars) for these changes: {}. Diff: {}",
        summary.to_summary_text(),
        truncate(&diff_text, 500)
    );

    let output = Command::new("gh")
        .args(["copilot", "suggest", "-t", "git:commit", &prompt])
        .current_dir(repo_path)
        .output()
        .context("Failed to run gh copilot suggest")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh copilot suggest failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = parse_copilot_output(&stdout);

    if message.is_empty() {
        bail!("GitHub Copilot returned empty response");
    }

    Ok(message)
}

/// Parse the output from gh copilot suggest to extract the commit message.
fn parse_copilot_output(output: &str) -> String {
    // gh copilot suggest outputs the suggestion — try to extract the actual message
    // The output format varies, so we try to be flexible
    let lines: Vec<&str> = output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    // Try to find the actual suggestion line (skip headers/prompts)
    for line in lines.iter().rev() {
        let cleaned = line.trim_start_matches(['>', '#', '-', '*', ' ']);
        if !cleaned.is_empty()
            && !cleaned.starts_with("Suggestion")
            && !cleaned.starts_with("?")
            && !cleaned.contains("copilot")
        {
            return cleaned.to_string();
        }
    }

    output.lines().last().unwrap_or("").trim().to_string()
}

/// Get the staged diff summary for prompt context.
fn get_diff_summary(repo_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "--staged", "--stat"])
        .current_dir(repo_path)
        .output()
        .context("Failed to get diff summary")?;

    if output.status.success() && !output.stdout.is_empty() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    // If nothing is staged, get the working directory diff
    let output = Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(repo_path)
        .output()
        .context("Failed to get diff summary")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Truncate a string to a maximum length.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
