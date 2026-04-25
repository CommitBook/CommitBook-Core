use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{build_resolve_prompt, strip_outer_code_fence, ConflictResolver};
use super::{clean_message, looks_like_diff_narration, truncate, CommitMessageProvider};
use crate::git::ChangesSummary;

const CLAUDE_TIMEOUT: Duration = Duration::from_secs(30);
const CLAUDE_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);

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
            "Write a single-line git commit message in imperative mood (max 72 chars, no quotes, no markdown, no prefix) describing what changed. Do not describe the diff itself or mention 'staged'/'unstaged'. Files: {}. Changes:\n{}",
            summary.to_summary_text(),
            truncate(&diff_summary, 500)
        );

        let repo_path = repo_path.to_path_buf();
        let output = tokio::task::spawn_blocking(move || {
            let child = Command::new("claude")
                .args(["-p", &prompt])
                .current_dir(&repo_path)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .context("Failed to start claude CLI")?;

            wait_with_timeout(child, CLAUDE_TIMEOUT)
                .context("claude CLI timed out")
        })
        .await
        .context("spawn_blocking panicked")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("claude CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let msg = clean_message(&raw);

        if msg.is_empty() {
            bail!("Empty response from claude CLI");
        }
        if looks_like_diff_narration(&msg) {
            bail!("claude CLI returned diff narration, not a commit message");
        }

        Ok(msg)
    }
}

#[async_trait]
impl ConflictResolver for ClaudeProvider {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn key(&self) -> &str {
        "claude"
    }

    fn is_available(&self) -> bool {
        which::which("claude").is_ok()
    }

    async fn resolve(
        &self,
        file_path: &Path,
        content_with_markers: &str,
        repo_path: &Path,
    ) -> Result<String> {
        let prompt = build_resolve_prompt(file_path, content_with_markers);
        let repo_path = repo_path.to_path_buf();
        let output = tokio::task::spawn_blocking(move || {
            let child = Command::new("claude")
                .args(["-p", &prompt])
                .current_dir(&repo_path)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .context("Failed to start claude CLI")?;
            wait_with_timeout(child, CLAUDE_RESOLVE_TIMEOUT).context("claude CLI timed out")
        })
        .await
        .context("spawn_blocking panicked")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("claude CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(strip_outer_code_fence(&raw))
    }
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;

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
                    let _ = child.wait();
                    bail!("Process timed out after {:?}", timeout);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => bail!("Error waiting for process: {}", e),
        }
    }
}
