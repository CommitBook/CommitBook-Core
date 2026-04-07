use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

/// Check if the git remote is reachable (with timeout).
pub fn check_remote_connectivity(repo_path: &Path) -> Result<bool> {
    let child = Command::new("git")
        .args(["ls-remote", "--exit-code", "--quiet", "origin"])
        .current_dir(repo_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to start git ls-remote")?;

    let start = std::time::Instant::now();
    let mut child = child;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) => {
                if start.elapsed() > REMOTE_TIMEOUT {
                    let _ = child.kill();
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return Ok(false),
        }
    }
}

/// Get the remote URL for the given remote name.
pub fn get_remote_url(repo_path: &Path, remote: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", remote])
        .current_dir(repo_path)
        .output()
        .context("Failed to get remote URL")?;

    if !output.status.success() {
        anyhow::bail!("Remote '{}' not found", remote);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
