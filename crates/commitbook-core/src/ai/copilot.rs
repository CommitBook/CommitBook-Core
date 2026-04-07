use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::{clean_message, truncate, CommitMessageProvider};
use crate::git::ChangesSummary;

const COPILOT_TIMEOUT: Duration = Duration::from_secs(15);

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
            .args(["copilot", "--version"])
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
            .args(["copilot", "suggest", "-t", "git:commit", &prompt])
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

/// Parse the output from gh copilot suggest to extract the commit message.
fn extract_message(output: &str) -> String {
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
        return clean_message(last);
    }

    String::new()
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
mod tests {
    use super::*;

    #[test]
    fn test_extract_message_simple() {
        assert_eq!(extract_message("Update README.md"), "Update README.md");
    }

    #[test]
    fn test_extract_message_with_prefix() {
        let output = "git commit -m \"Fix typo in docs\"";
        assert_eq!(extract_message(output), "Fix typo in docs");
    }

    #[test]
    fn test_extract_message_multiline() {
        let output = "? What would you like to do?\n> Suggestion\n\nAdd new feature to parser";
        assert_eq!(extract_message(output), "Add new feature to parser");
    }

    #[test]
    fn test_extract_message_empty() {
        assert_eq!(extract_message(""), "");
    }

    #[test]
    fn test_extract_message_skip_headers() {
        let output = "# Some header\n? prompt\nSuggestion from copilot\nFix login bug";
        assert_eq!(extract_message(output), "Fix login bug");
    }
}
