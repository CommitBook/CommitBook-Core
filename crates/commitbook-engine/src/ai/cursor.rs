use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{
    build_resolve_prompt, strip_outer_code_fence, ConflictResolution, ConflictResolver,
};
use super::wait_with_timeout;
use crate::git::GitConflict;

const CURSOR_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct CursorProvider;

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
        let child = Command::new("cursor-agent")
            .args(["-p", &prompt])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start cursor-agent CLI")?;

        let output = wait_with_timeout(child, CURSOR_RESOLVE_TIMEOUT)
            .context("cursor-agent CLI timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("cursor-agent CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let resolved = strip_outer_code_fence(&raw);
        if resolved.trim().is_empty() {
            bail!("Empty resolution from cursor-agent CLI");
        }
        Ok(ConflictResolution::WriteContent(resolved))
    }
}
