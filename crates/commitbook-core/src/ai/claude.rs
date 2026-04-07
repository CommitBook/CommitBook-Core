use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::{clean_message, truncate, CommitMessageProvider};
use crate::git::ChangesSummary;

const CLAUDE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ClaudeProvider;

#[async_trait]
impl CommitMessageProvider for ClaudeProvider {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn key(&self) -> &str {
        "claude-cli"
    }

    fn is_available(&self) -> bool {
        which::which("claude").is_ok()
    }

    async fn generate(&self, summary: &ChangesSummary, repo_path: &Path) -> Result<String> {
        let diff_summary = crate::git::GitRepo::open(repo_path)
            .and_then(|r| r.diff_summary())
            .unwrap_or_default();

        let prompt = format!(
            "Write a concise one-line git commit message (max 72 chars, no quotes) for these changes: {}. Diff stats:\n{}",
            summary.to_summary_text(),
            truncate(&diff_summary, 500)
        );

        let child = Command::new("claude")
            .args(["-p", &prompt])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start claude CLI")?;

        let output = wait_with_timeout(child, CLAUDE_TIMEOUT)
            .context("claude CLI timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("claude CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let msg = clean_message(&raw);

        if msg.is_empty() {
            bail!("Empty response from claude CLI");
        }

        Ok(msg)
    }
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
