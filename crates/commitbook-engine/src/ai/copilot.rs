use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{
    build_resolve_prompt, finalize_resolved_text, strip_outer_code_fence, ConflictResolution,
    ConflictResolver,
};
use super::{
    clean_message, looks_like_diff_narration, truncate, wait_with_timeout, CommitMessageProvider,
};
use crate::git::{ChangesSummary, GitConflict};

const COPILOT_TIMEOUT: Duration = Duration::from_secs(10);
const COPILOT_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct CopilotProvider;

fn command(repo_path: &Path, prompt: &str) -> Command {
    let mut command = Command::new("gh");
    command
        .args([
            "copilot",
            "--",
            "-p",
            prompt,
            "--silent",
            "--no-color",
            "--no-custom-instructions",
        ])
        .current_dir(repo_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

fn has_cli_diagnostics(output: &str) -> bool {
    let lines: Vec<&str> = output.lines().map(str::trim).collect();
    let usage_block = lines.iter().any(|line| {
        line.starts_with("Total usage est:")
            || line.starts_with("Total duration (API):")
            || line.starts_with("Total duration (wall):")
            || line.starts_with("Total code changes:")
            || *line == "Usage by model:"
    });
    let tool_heading = lines
        .iter()
        .any(|line| line.starts_with("✓ ") || line.starts_with("✗ "));
    let tool_detail = lines
        .iter()
        .any(|line| line.starts_with("$ ") || line.starts_with("↪ "));
    usage_block || (tool_heading && tool_detail)
}

fn parse_response(output: &str) -> Result<String> {
    if has_cli_diagnostics(output) {
        bail!("GitHub Copilot response contained CLI diagnostics");
    }

    let response = strip_outer_code_fence(output);
    if response.trim().is_empty() {
        bail!("GitHub Copilot returned empty response");
    }
    Ok(response)
}

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
            .args(["copilot", "--", "-v"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn generate(&self, summary: &ChangesSummary, repo_path: &Path) -> Result<String> {
        let diff_text = crate::git::GitRepo::open(repo_path)
            .and_then(|r| r.diff_summary())
            .unwrap_or_default();
        let prompt = format!(
            "Write a single-line git commit message in imperative mood (max 72 chars, no quotes, no markdown, no prefix) describing what changed. Do not describe the diff itself or mention 'staged'/'unstaged'. Files: {}. Changes:\n{}",
            summary.to_summary_text(),
            truncate(&diff_text, 500)
        );

        let child = command(repo_path, &prompt)
            .spawn()
            .context("Failed to run gh copilot")?;

        let output = wait_with_timeout(child, COPILOT_TIMEOUT).context("gh copilot timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gh copilot failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let response = parse_response(&stdout)?;
        let message = clean_message(&response);
        if message.is_empty() {
            bail!("GitHub Copilot returned empty commit message");
        }
        if looks_like_diff_narration(&message) {
            bail!("GitHub Copilot returned diff narration, not a commit message");
        }

        Ok(message)
    }
}

#[async_trait]
impl ConflictResolver for CopilotProvider {
    fn name(&self) -> &str {
        "GitHub Copilot"
    }

    fn key(&self) -> &str {
        "copilot"
    }

    fn is_available(&self) -> bool {
        if which::which("gh").is_err() {
            return false;
        }
        Command::new("gh")
            .args(["copilot", "--", "-v"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn resolve(
        &self,
        conflict: &GitConflict,
        repo_path: &Path,
    ) -> Result<ConflictResolution> {
        let prompt = build_resolve_prompt(conflict)?;
        let child = command(repo_path, &prompt)
            .spawn()
            .context("Failed to run gh copilot")?;

        let output =
            wait_with_timeout(child, COPILOT_RESOLVE_TIMEOUT).context("gh copilot timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gh copilot failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let resolved = parse_response(&raw)?;
        finalize_resolved_text(&resolved, "GitHub Copilot")
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

#[cfg(test)]
#[path = "copilot_tests.rs"]
mod tests;
