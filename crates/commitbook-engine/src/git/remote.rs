use anyhow::{bail, Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};
use std::path::Path;
use url::Url;

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
    /// Actual hostname, lowercased and without credentials; None for local paths.
    pub host: Option<String>,
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
    let local = local_remote_path(trimmed);
    let (host, path, ssh) = if let Some(path) = local.as_deref() {
        (None, path, false)
    } else if let Some((scheme, rest)) = trimmed.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let host = if let Some(ipv6) = host.strip_prefix('[') {
            ipv6.split(']').next().unwrap_or(ipv6)
        } else {
            host.split(':').next().unwrap_or(host)
        };
        (
            Some(host),
            path,
            scheme == "ssh" || scheme.starts_with("git+ssh"),
        )
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
        host: host.map(str::to_ascii_lowercase),
        owner,
        repo: repo.to_string(),
        ssh,
    })
}

/// Normalize absolute Windows paths independently of the host OS. A drive
/// prefix is not an SSH host or a parent directory in the repository identity.
/// Only Windows paths get backslash normalization; POSIX names keep theirs.
fn local_remote_path(url: &str) -> Option<String> {
    fn windows_path(path: &str) -> Option<String> {
        let bytes = path.as_bytes();
        (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
        .then(|| path[2..].replace('\\', "/"))
    }
    if url
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        let path = &url[5..];
        return Some(
            windows_path(path.trim_start_matches('/'))
                .unwrap_or_else(|| path.strip_prefix("//").unwrap_or(path).to_string()),
        );
    }
    windows_path(url)
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

/// A URL suitable for UI and credential callbacks. HTTP userinfo and any
/// URL password are removed; SSH usernames (normally `git`) are preserved.
pub fn credential_free_url(remote_url: &str) -> Result<String> {
    let remote_url = remote_url.trim();
    if remote_url.is_empty() || remote_url.chars().any(char::is_control) {
        bail!("Git remote URL must be non-empty and contain no control characters");
    }
    if let Ok(mut parsed) = Url::parse(remote_url) {
        if parsed.scheme() == "http" || parsed.scheme() == "https" {
            parsed
                .set_username("")
                .map_err(|_| anyhow::anyhow!("Invalid Git URL"))?;
        }
        parsed
            .set_password(None)
            .map_err(|_| anyhow::anyhow!("Invalid Git URL"))?;
        parsed.set_query(None);
        parsed.set_fragment(None);
        return Ok(parsed.to_string());
    }
    parse_remote_url(remote_url)?;
    Ok(remote_url.to_string())
}

/// Reject credentials embedded in clone input; the host callback owns them.
pub fn validate_clone_url(remote_url: &str) -> Result<()> {
    credential_free_url(remote_url)?;
    if let Ok(parsed) = Url::parse(remote_url) {
        if parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || (matches!(parsed.scheme(), "http" | "https") && !parsed.username().is_empty())
        {
            bail!("Put Git credentials in the host callback, not the remote URL");
        }
    }
    Ok(())
}

/// Compare remote endpoints, not transport schemes or usernames. GitHub
/// names are case-insensitive; generic hosts retain path case.
pub fn same_remote(a: &str, b: &str) -> bool {
    fn key(remote_url: &str) -> Option<(String, String)> {
        if let Ok(parsed) = Url::parse(remote_url) {
            if parsed.scheme() == "file" {
                let path = parsed.to_file_path().ok()?.canonicalize().ok()?;
                return Some(("local".into(), path.to_string_lossy().into_owned()));
            }
            if let Some(host) = parsed.host_str() {
                let port = parsed.port().filter(|port| {
                    !matches!(
                        (parsed.scheme(), *port),
                        ("https", 443) | ("http", 80) | ("ssh", 22)
                    )
                });
                let authority = match port {
                    Some(port) => format!("{}:{port}", host.to_ascii_lowercase()),
                    None => host.to_ascii_lowercase(),
                };
                let path = parsed.path().trim_end_matches('/').trim_end_matches(".git");
                let path = if host.eq_ignore_ascii_case("github.com") {
                    path.to_ascii_lowercase()
                } else {
                    path.to_string()
                };
                return Some((authority, path));
            }
        }
        let identity = parse_remote_url(remote_url).ok()?;
        if let Some(host) = identity.host {
            let path = format!("/{}/{}", identity.owner, identity.repo);
            return Some((
                host.clone(),
                if host == "github.com" {
                    path.to_ascii_lowercase()
                } else {
                    path
                },
            ));
        }
        let path = std::path::Path::new(remote_url).canonicalize().ok()?;
        Some(("local".into(), path.to_string_lossy().into_owned()))
    }
    key(a).zip(key(b)).is_some_and(|(a, b)| a == b)
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
