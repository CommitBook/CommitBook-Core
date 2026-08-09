use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{build_resolve_prompt, strip_outer_code_fence, ConflictResolver};
use super::{
    clean_message, looks_like_diff_narration, truncate, wait_with_timeout, CommitMessageProvider,
};
use crate::git::ChangesSummary;

const CODEX_TIMEOUT: Duration = Duration::from_secs(30);
const CODEX_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);

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

        let output = wait_with_timeout(child, CODEX_TIMEOUT).context("codex CLI timed out")?;

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

#[async_trait]
impl ConflictResolver for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
    }

    fn key(&self) -> &str {
        "codex"
    }

    fn is_available(&self) -> bool {
        which::which("codex").is_ok()
    }

    async fn resolve(
        &self,
        file_path: &Path,
        content_with_markers: &str,
        repo_path: &Path,
    ) -> Result<String> {
        let prompt = build_resolve_prompt(file_path, content_with_markers);
        let child = Command::new("codex")
            .args(["--quiet", &prompt])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start codex CLI")?;

        let output =
            wait_with_timeout(child, CODEX_RESOLVE_TIMEOUT).context("codex CLI timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("codex CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let resolved = strip_outer_code_fence(&raw);
        if resolved.trim().is_empty() {
            bail!("Empty resolution from codex CLI");
        }
        Ok(resolved)
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
