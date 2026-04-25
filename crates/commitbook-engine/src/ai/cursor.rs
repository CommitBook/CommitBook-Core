use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::conflict::{build_resolve_prompt, strip_outer_code_fence, ConflictResolver};

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
        file_path: &Path,
        content_with_markers: &str,
        repo_path: &Path,
    ) -> Result<String> {
        let prompt = build_resolve_prompt(file_path, content_with_markers);
        let child = Command::new("cursor-agent")
            .args(["-p", &prompt])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start cursor-agent CLI")?;

        let output = wait_with_timeout(child, CURSOR_RESOLVE_TIMEOUT).context("cursor-agent CLI timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("cursor-agent CLI failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(strip_outer_code_fence(&raw))
    }
}

fn wait_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    let mut child = child;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().context("Failed to get process output");
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
