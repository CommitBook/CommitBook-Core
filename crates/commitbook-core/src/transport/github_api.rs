use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};

use crate::domain::transport::{RemoteDocument, RepoDescriptor, WriteFileInput, WriteFileResult};

const GITHUB_API_BASE: &str = "https://api.github.com";

/// Build common headers for GitHub API requests.
pub fn github_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("CommitBook/0.1"));
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2022-11-28"),
    );
    Ok(headers)
}

/// Build a reqwest client with GitHub headers.
pub fn github_client(token: &str) -> Result<reqwest::Client> {
    let headers = github_headers(token)?;
    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

// --- API Response Types ---

#[derive(Debug, Deserialize)]
pub struct GithubRepo {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
    pub private: bool,
    pub owner: GithubOwner,
}

#[derive(Debug, Deserialize)]
pub struct GithubOwner {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubTreeEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubTreeResponse {
    pub sha: String,
    pub tree: Vec<GithubTreeEntry>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
pub struct GithubBlobResponse {
    pub content: String,
    pub encoding: String,
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub object: GithubRefObject,
}

#[derive(Debug, Deserialize)]
pub struct GithubRefObject {
    pub sha: String,
    #[serde(rename = "type")]
    pub object_type: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubCommit {
    pub sha: String,
}

#[derive(Debug, Serialize)]
struct CreateBlobRequest {
    content: String,
    encoding: String,
}

#[derive(Debug, Deserialize)]
struct CreateBlobResponse {
    sha: String,
}

#[derive(Debug, Serialize)]
struct CreateTreeRequest {
    base_tree: String,
    tree: Vec<TreeEntry>,
}

#[derive(Debug, Serialize)]
struct TreeEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    entry_type: String,
    sha: String,
}

#[derive(Debug, Deserialize)]
struct CreateTreeResponse {
    sha: String,
}

#[derive(Debug, Serialize)]
struct CreateCommitRequest {
    message: String,
    tree: String,
    parents: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CreateCommitResponse {
    sha: String,
}

#[derive(Debug, Serialize)]
struct UpdateRefRequest {
    sha: String,
    force: bool,
}

// --- API Functions ---

/// Validate token by fetching the authenticated user.
pub async fn validate_token(client: &reqwest::Client) -> Result<()> {
    let resp = client
        .get(format!("{GITHUB_API_BASE}/user"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("GitHub token validation failed: {}", resp.status());
    }
    Ok(())
}

/// List repositories accessible to the authenticated user.
pub async fn list_repos(client: &reqwest::Client) -> Result<Vec<RepoDescriptor>> {
    let mut all_repos = Vec::new();
    let mut page = 1;

    loop {
        let resp: Vec<GithubRepo> = client
            .get(format!(
                "{GITHUB_API_BASE}/user/repos?per_page=100&page={page}&sort=updated"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if resp.is_empty() {
            break;
        }

        for repo in &resp {
            all_repos.push(RepoDescriptor {
                id: repo.id.to_string(),
                provider: "github".to_string(),
                owner: repo.owner.login.clone(),
                name: repo.name.clone(),
                default_branch: repo.default_branch.clone(),
                private: repo.private,
            });
        }

        if resp.len() < 100 {
            break;
        }
        page += 1;
    }

    Ok(all_repos)
}

/// List file paths in a repo by fetching the git tree recursively.
pub async fn list_files(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<Vec<String>> {
    let resp: GithubTreeResponse = client
        .get(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/trees/{branch}?recursive=1"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let files: Vec<String> = resp
        .tree
        .into_iter()
        .filter(|e| {
            e.entry_type == "blob"
                && (e.path.ends_with(".md") || e.path.ends_with(".markdown"))
        })
        .map(|e| e.path)
        .collect();

    Ok(files)
}

/// Read a single file from the repo via the Contents API.
pub async fn read_file(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
    path: &str,
) -> Result<RemoteDocument> {
    let url = format!("{GITHUB_API_BASE}/repos/{owner}/{repo}/contents/{path}?ref={branch}");
    let resp = client
        .get(&url)
        .header(ACCEPT, "application/vnd.github.raw+json")
        .send()
        .await?;

    if !resp.status().is_success() {
        bail!("Failed to read {path}: {}", resp.status());
    }

    let content = resp.text().await?;

    // Get the blob SHA for this file.
    let blob_sha = get_file_blob_sha(client, owner, repo, branch, path)
        .await
        .unwrap_or_default();

    Ok(RemoteDocument {
        path: path.to_string(),
        content,
        revision: blob_sha,
    })
}

/// Get the blob SHA for a specific file path.
async fn get_file_blob_sha(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
    path: &str,
) -> Result<String> {
    #[derive(Deserialize)]
    struct FileInfo {
        sha: String,
    }
    let resp: FileInfo = client
        .get(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/contents/{path}?ref={branch}"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.sha)
}

/// Get the HEAD commit SHA for a branch.
pub async fn get_head(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<String> {
    let resp: GithubRef = client
        .get(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/ref/heads/{branch}"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.object.sha)
}

/// Create an atomic multi-file commit using the Git Data API.
///
/// Steps: create blobs → create tree → create commit → update ref.
pub async fn create_tree_and_commit(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
    inputs: Vec<WriteFileInput>,
) -> Result<Vec<WriteFileResult>> {
    // 1. Get current HEAD.
    let head_sha = get_head(client, owner, repo, branch).await?;

    // 2. Get the current tree SHA.
    let commit_resp: serde_json::Value = client
        .get(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/commits/{head_sha}"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let base_tree_sha = commit_resp["tree"]["sha"]
        .as_str()
        .context("Missing tree SHA")?
        .to_string();

    // 3. Create blobs for each file.
    let mut tree_entries = Vec::new();
    for input in &inputs {
        let blob: CreateBlobResponse = client
            .post(format!(
                "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/blobs"
            ))
            .json(&CreateBlobRequest {
                content: input.content.clone(),
                encoding: "utf-8".to_string(),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        tree_entries.push(TreeEntry {
            path: input.path.clone(),
            mode: "100644".to_string(),
            entry_type: "blob".to_string(),
            sha: blob.sha,
        });
    }

    // 4. Create new tree.
    let new_tree: CreateTreeResponse = client
        .post(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/trees"
        ))
        .json(&CreateTreeRequest {
            base_tree: base_tree_sha,
            tree: tree_entries,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // 5. Create commit.
    let message = if inputs.len() == 1 {
        inputs[0].message.clone()
    } else {
        format!("Update {} files via CommitBook", inputs.len())
    };

    let new_commit: CreateCommitResponse = client
        .post(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/commits"
        ))
        .json(&CreateCommitRequest {
            message,
            tree: new_tree.sha,
            parents: vec![head_sha],
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // 6. Update branch ref.
    client
        .patch(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/refs/heads/{branch}"
        ))
        .json(&UpdateRefRequest {
            sha: new_commit.sha.clone(),
            force: false,
        })
        .send()
        .await?
        .error_for_status()?;

    let results = inputs
        .iter()
        .map(|input| WriteFileResult {
            path: input.path.clone(),
            new_revision: new_commit.sha.clone(),
        })
        .collect();

    Ok(results)
}

/// Delete a file via the Contents API.
pub async fn delete_file(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
    path: &str,
    message: &str,
) -> Result<()> {
    let blob_sha = get_file_blob_sha(client, owner, repo, branch, path).await?;

    #[derive(Serialize)]
    struct DeleteRequest {
        message: String,
        sha: String,
        branch: String,
    }

    client
        .delete(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/contents/{path}"
        ))
        .json(&DeleteRequest {
            message: message.to_string(),
            sha: blob_sha,
            branch: branch.to_string(),
        })
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}
