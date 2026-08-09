use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{build_resolve_prompt, strip_outer_code_fence, ConflictResolver};
use super::wait_with_timeout;

const GEMINI_RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct GeminiProvider;

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
        file_path: &Path,
        content_with_markers: &str,
        repo_path: &Path,
    ) -> Result<String> {
        let prompt = build_resolve_prompt(file_path, content_with_markers);
        let child = Command::new("gemini")
            .args(["-p", &prompt])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start gemini CLI")?;

        let output =
            wait_with_timeout(child, GEMINI_RESOLVE_TIMEOUT).context("gemini CLI timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gemini CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let resolved = strip_outer_code_fence(&raw);
        if resolved.trim().is_empty() {
            bail!("Empty resolution from gemini CLI");
        }
        Ok(resolved)
    }
}
