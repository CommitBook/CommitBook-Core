use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{
    build_resolve_prompt, finalize_resolved_text, ConflictResolution, ConflictResolver,
};
use crate::git::GitConflict;

const CURSOR_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct CursorProvider;

/// Print mode reads the prompt from stdin; text output avoids JSON envelopes.
pub(crate) fn command(repo_path: &Path) -> Command {
    let mut command = Command::new("cursor-agent");
    command
        .args(["-p", "--output-format", "text"])
        .current_dir(repo_path);
    command
}

#[async_trait]
impl ConflictResolver for CursorProvider {
    fn name(&self) -> &str {
        "Cursor Agent"
    }

    fn key(&self) -> &str {
        "cursor"
    }

    fn is_available(&self) -> bool {
        which::which("cursor-agent").is_ok()
    }

    async fn resolve(
        &self,
        conflict: &GitConflict,
        repo_path: &Path,
    ) -> Result<ConflictResolution> {
        let prompt = build_resolve_prompt(conflict)?;
        let repo_path = repo_path.to_path_buf();
        let output = tokio::task::spawn_blocking(move || {
            super::run_with_prompt(&mut command(&repo_path), &prompt, CURSOR_RESOLVE_TIMEOUT)
        })
        .await
        .context("spawn_blocking panicked")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("cursor-agent CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        finalize_resolved_text(&raw, "cursor-agent CLI")
    }
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
