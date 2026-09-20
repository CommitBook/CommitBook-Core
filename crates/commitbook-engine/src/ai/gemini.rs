use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{
    build_resolve_prompt, finalize_resolved_text, ConflictResolution, ConflictResolver,
};
use crate::git::GitConflict;

const GEMINI_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);
/// `-p` carries only this fixed instruction; the conflict itself travels on
/// stdin so large files never hit the process-argument limit.
pub(crate) const GEMINI_RESOLVE_INSTRUCTION: &str =
    "Resolve the structured conflict supplied on stdin and output only final contents.";

pub struct GeminiProvider;

pub(crate) fn command(repo_path: &Path) -> Command {
    let mut command = Command::new("gemini");
    command
        .args(["-p", GEMINI_RESOLVE_INSTRUCTION])
        .current_dir(repo_path);
    command
}

#[async_trait]
impl ConflictResolver for GeminiProvider {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    fn key(&self) -> &str {
        "gemini"
    }

    fn is_available(&self) -> bool {
        which::which("gemini").is_ok()
    }

    async fn resolve(
        &self,
        conflict: &GitConflict,
        repo_path: &Path,
    ) -> Result<ConflictResolution> {
        let prompt = build_resolve_prompt(conflict)?;
        let repo_path = repo_path.to_path_buf();
        let output = tokio::task::spawn_blocking(move || {
            super::run_with_prompt(&mut command(&repo_path), &prompt, GEMINI_RESOLVE_TIMEOUT)
        })
        .await
        .context("spawn_blocking panicked")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gemini CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        finalize_resolved_text(&raw, "gemini CLI")
    }
}

#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tests;
