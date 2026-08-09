use anyhow::{Context, Result};
use git2::{Direction, Repository};
use std::path::Path;

/// Check if the git remote is reachable.
///
/// Uses libgit2 to open the repository and connect to the `origin` remote
/// in fetch direction. Returns `Ok(false)` if the remote exists but is
/// unreachable; returns `Err` if the path is not a git repository.
pub fn check_remote_connectivity(repo_path: &Path) -> Result<bool> {
    let repo = Repository::open(repo_path).context("Failed to open repository")?;
    let mut remote = match repo.find_remote("origin") {
        Ok(r) => r,
        Err(_) => return Ok(false),
    };
    Ok(remote.connect(Direction::Fetch).is_ok())
}

/// Get the remote URL for the given remote name.
pub fn get_remote_url(repo_path: &Path, remote: &str) -> Result<String> {
    let repo = Repository::open(repo_path).context("Failed to open repository")?;
    let r = repo
        .find_remote(remote)
        .with_context(|| format!("Remote '{}' not found", remote))?;
    r.url()
        .map(|s| s.to_string())
        .with_context(|| format!("Remote '{}' has no URL", remote))
}

/// List the names of all configured remotes in the repo.
pub fn list_remote_names(repo_path: &Path) -> Result<Vec<String>> {
    let repo = Repository::open(repo_path).context("Failed to open repository")?;
    let remotes = repo.remotes().context("Failed to read git remotes")?;
    Ok(remotes.iter().flatten().map(|s| s.to_string()).collect())
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
