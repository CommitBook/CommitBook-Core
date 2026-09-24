use anyhow::{bail, Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Hosting service a remote URL points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Github,
    Gitlab,
    Codeberg,
    GenericGit,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Codeberg => "codeberg",
            Self::GenericGit => "generic_git",
        }
    }
}

/// Where a remote lives, derived from its URL rather than stored in config
/// so it can never go stale after a rename or `git remote set-url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteIdentity {
    pub provider: Provider,
    /// Everything before the repository name on a host (`user`, or
    /// `group/sub` on GitLab); the parent folder for a local path. Empty when
    /// there is none.
    pub owner: String,
    pub repo: String,
    /// SSH transport (`git@host:path` or `ssh://`).
    pub ssh: bool,
}

/// Parse a Git remote URL: `https://`, `ssh://`, scp-like `git@host:path`,
/// or a local path. Embedded credentials are ignored.
pub fn parse_remote_url(url: &str) -> Result<RemoteIdentity> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        bail!("Remote URL is empty");
    }
    let (host, path, ssh) = if let Some((scheme, rest)) = trimmed.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if scheme == "file" {
            (None, rest, false)
        } else {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = authority.rsplit('@').next().unwrap_or(authority);
            let host = host.split(':').next().unwrap_or(host);
            (
                Some(host),
                path,
                scheme == "ssh" || scheme.starts_with("git+ssh"),
            )
        }
    } else if let Some((authority, path)) = scp_like(trimmed) {
        let host = authority.rsplit('@').next().unwrap_or(authority);
        (Some(host), path, true)
    } else {
        (None, trimmed, false)
    };

    let mut segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .collect();
    let last = segments
        .pop()
        .with_context(|| format!("Remote URL has no repository name: {url}"))?;
    let repo = last.strip_suffix(".git").unwrap_or(last);
    if repo.is_empty() {
        bail!("Remote URL has no repository name: {url}");
    }
    let provider = match host.map(str::to_ascii_lowercase).as_deref() {
        Some("github.com") | Some("www.github.com") => Provider::Github,
        Some("gitlab.com") | Some("www.gitlab.com") => Provider::Gitlab,
        Some("codeberg.org") | Some("www.codeberg.org") => Provider::Codeberg,
        _ => Provider::GenericGit,
    };
    let owner = if host.is_some() {
        segments.join("/")
    } else {
        segments.last().copied().unwrap_or_default().to_string()
    };
    Ok(RemoteIdentity {
        provider,
        owner,
        repo: repo.to_string(),
        ssh,
    })
}

/// Split scp-like `[user@]host:path`, which Git treats as SSH. A colon after
/// the first slash, or a leading `/` or `.`, means a local path instead.
fn scp_like(url: &str) -> Option<(&str, &str)> {
    if url.starts_with('/') || url.starts_with('.') {
        return None;
    }
    let (authority, path) = url.split_once(':')?;
    if authority.is_empty() || authority.contains('/') {
        return None;
    }
    Some((authority, path))
}

/// Identity of the named remote, parsed from its configured URL.
pub fn remote_identity(repo_path: &Path, remote: &str) -> Result<RemoteIdentity> {
    parse_remote_url(&get_remote_url(repo_path, remote)?)
}

/// Get the remote URL for the given remote name.
pub fn get_remote_url(repo_path: &Path, remote: &str) -> Result<String> {
    let repo = Repository::open(repo_path).context("Failed to open repository")?;
    let r = repo
        .find_remote(remote)
        .with_context(|| format!("Remote '{}' not found", remote))?;
    let url = r
        .url()
        .with_context(|| format!("Remote '{}' URL is not valid UTF-8", remote))?;
    if url.is_empty() {
        bail!("Remote '{}' has no URL", remote);
    }
    Ok(url.to_string())
}

/// List the names of all configured remotes in the repo.
pub fn list_remote_names(repo_path: &Path) -> Result<Vec<String>> {
    let repo = Repository::open(repo_path).context("Failed to open repository")?;
    let remotes = repo.remotes().context("Failed to read git remotes")?;
    remotes
        .iter()
        .map(|name| {
            name.context("Remote name is not valid UTF-8")?
                .map(str::to_string)
                .context("Remote disappeared while reading its name")
        })
        .collect()
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
