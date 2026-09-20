use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{
    build_resolve_prompt, finalize_resolved_text, ConflictResolution, ConflictResolver,
};
use super::{
    clean_message, looks_like_diff_narration, run_with_prompt, truncate, CommitMessageProvider,
};
use crate::git::{ChangesSummary, GitConflict};

const CODEX_TIMEOUT: Duration = Duration::from_secs(30);
const CODEX_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct CodexProvider;

fn command(repo_path: &Path, output_path: &Path) -> Command {
    let mut command = Command::new("codex");
    command
        .args([
            "exec",
            "--ephemeral",
            "--sandbox",
            "read-only",
            "--color",
            "never",
            "--output-last-message",
        ])
        .arg(output_path)
        .arg("-")
        .current_dir(repo_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

fn run(prompt: &str, repo_path: &Path, timeout: Duration) -> Result<String> {
    let output_file =
        tempfile::NamedTempFile::new().context("Failed to create Codex output file")?;
    let output = run_with_prompt(&mut command(repo_path, output_file.path()), prompt, timeout)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("codex CLI failed: {}", stderr.trim());
    }

    std::fs::read_to_string(output_file.path())
        .context("Failed to read the final response from codex CLI")
}

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

        let raw = run(&prompt, repo_path, CODEX_TIMEOUT)?;
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
        conflict: &GitConflict,
        repo_path: &Path,
    ) -> Result<ConflictResolution> {
        let prompt = build_resolve_prompt(conflict)?;
        let repo_path = repo_path.to_path_buf();
        let raw =
            tokio::task::spawn_blocking(move || run(&prompt, &repo_path, CODEX_RESOLVE_TIMEOUT))
                .await
                .context("spawn_blocking panicked")??;
        finalize_resolved_text(&raw, "codex CLI")
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
