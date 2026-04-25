use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::{clean_message, looks_like_diff_narration, truncate, CommitMessageProvider};
use crate::git::ChangesSummary;

const CODEX_TIMEOUT: Duration = Duration::from_secs(30);

pub struct CodexProvider;

#[async_trait]
impl CommitMessageProvider for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
    }

    fn key(&self) -> &str {
        "codex-cli"
    }

    fn is_available(&self) -> bool {
        which::which("codex").is_ok()
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

        let child = Command::new("codex")
            .args(["--quiet", &prompt])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start codex CLI")?;

        let output = wait_with_timeout(child, CODEX_TIMEOUT)
            .context("codex CLI timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("codex CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let msg = clean_message(&raw);

        if msg.is_empty() {
            bail!("Empty response from codex CLI");
        }
        if looks_like_diff_narration(&msg) {
            bail!("codex CLI returned diff narration, not a commit message");
        }

        Ok(msg)
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
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
                    bail!("Process timed out after {:?}", timeout);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => bail!("Error waiting for process: {}", e),
        }
    }
}
