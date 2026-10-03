//! GitHub PAT validation + repo enumeration via the GitHub REST API.

use serde::Deserialize;
use std::time::Duration;

use crate::errors::{CommitBookError, Result};
use crate::types::RepoInfo;

const GITHUB_API: &str = "https://api.github.com";
const USER_AGENT: &str = "commitbook-ffi/0.5";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct GithubRepo {
    name: String,
    default_branch: Option<String>,
    private: bool,
    owner: GithubOwner,
}

#[derive(Debug, Deserialize)]
struct GithubOwner {
    login: String,
}

/// Build a reqwest client for GitHub API calls. Callers making several
/// requests should build one and reuse it so the connection pool is shared.
pub(crate) fn github_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| CommitBookError::transport(format!("HTTP client init: {e}")))
}

/// Fetch the authenticated user's repos. Used for both PAT validation
/// (returns the list as proof the token works) and as the input to
/// `discover_commitbooks` filtering.
pub(crate) async fn fetch_user_repos(token: &str) -> Result<Vec<RepoInfo>> {
    let client = github_client()?;

    let mut all = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!("{GITHUB_API}/user/repos?per_page=100&sort=updated&page={page}");
        let resp = client
            .get(&url)
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| CommitBookError::transport(format!("GitHub /user/repos: {e}")))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CommitBookError::auth("GitHub token rejected (401)"));
        }
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(CommitBookError::auth(
                "GitHub token forbidden (403): check scopes (need `repo`)",
            ));
        }
        if !resp.status().is_success() {
            return Err(CommitBookError::transport(format!(
                "GitHub returned {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }

        let body: Vec<GithubRepo> = resp
            .json()
            .await
            .map_err(|e| CommitBookError::transport(format!("Parse repos: {e}")))?;

        if body.is_empty() {
            break;
        }
        let count = body.len();
        for r in body {
            all.push(RepoInfo {
                remote_url: format!("https://github.com/{}/{}.git", r.owner.login, r.name),
                name: r.name,
                default_branch: r.default_branch.unwrap_or_else(|| "main".into()),
                is_private: r.private,
            });
        }
        if count < 100 {
            break;
        }
        page += 1;
        if page > 50 {
            // Safety limit: 5000 repos.
            break;
        }
    }
    Ok(all)
}

/// Probe a single repo for the presence of a `.CommitBook/` directory at the
/// root tree using a caller-provided client, so a batch of probes shares one
/// connection pool. Returns `true` if found, `false` if not.
pub(crate) async fn has_dot_commitbook_with_client(
    client: &reqwest::Client,
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<bool> {
    let url = format!("{GITHUB_API}/repos/{owner}/{repo}/contents/.CommitBook");
    let resp = client
        .get(&url)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| CommitBookError::transport(format!("GitHub contents probe: {e}")))?;

    Ok(resp.status() == reqwest::StatusCode::OK)
}
