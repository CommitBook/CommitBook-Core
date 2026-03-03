use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Check if the git remote is reachable.
pub fn check_remote_connectivity(repo_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["ls-remote", "--exit-code", "--quiet", "origin"])
        .current_dir(repo_path)
        .output()
        .context("Failed to execute git ls-remote")?;

    Ok(output.status.success())
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
