use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::sync::Mutex;

use crate::domain::transport::{
    RemoteDocument, RemoteTransport, RepoDescriptor, WriteFileInput, WriteFileResult,
};

use super::github_api;

/// GitHub App transport — uses backend-minted installation access tokens.
///
/// On each operation, checks if the current token is still valid. If expired
/// or missing, fetches a new one from the CommitBook backend.
pub struct GithubAppTransport {
    backend_url: String,
    session_token: String,
    workspace_id: String,
    owner: String,
    repo: String,
    /// Cached installation token + expiry.
    cached_token: Mutex<Option<CachedInstallationToken>>,
}

struct CachedInstallationToken {
    token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl GithubAppTransport {
    pub fn new(
        backend_url: String,
        session_token: String,
        workspace_id: String,
        owner: String,
        repo: String,
    ) -> Self {
        Self {
            backend_url,
            session_token,
            workspace_id,
            owner,
            repo,
            cached_token: Mutex::new(None),
        }
    }

    /// Get a valid installation token, refreshing from backend if needed.
    async fn get_token(&self) -> Result<String> {
        // Check cache first.
        {
            let cache = self.cached_token.lock().unwrap();
            if let Some(cached) = cache.as_ref() {
                let now = chrono::Utc::now();
                // Use token if it has at least 60 seconds left.
                if cached.expires_at > now + chrono::Duration::seconds(60) {
                    return Ok(cached.token.clone());
                }
            }
        }

        // Fetch new token from backend.
        let client = reqwest::Client::new();
        let url = format!(
            "{}/workspaces/{}/token",
            self.backend_url, self.workspace_id
        );

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.session_token))
            .send()
            .await
            .context("Failed to reach CommitBook backend")?;

        if !resp.status().is_success() {
            bail!(
                "Backend token minting failed: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }

        #[derive(serde::Deserialize)]
        struct TokenResponse {
            token: String,
            expires_at: String,
        }

        let token_resp: TokenResponse = resp.json().await?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&token_resp.expires_at)
            .context("Invalid expires_at format")?
            .with_timezone(&chrono::Utc);

        // Cache the new token.
        {
            let mut cache = self.cached_token.lock().unwrap();
            *cache = Some(CachedInstallationToken {
                token: token_resp.token.clone(),
                expires_at,
            });
        }

        Ok(token_resp.token)
    }

    async fn client(&self) -> Result<reqwest::Client> {
        let token = self.get_token().await?;
        github_api::github_client(&token)
    }
}

#[async_trait]
impl RemoteTransport for GithubAppTransport {
    async fn validate(&self) -> Result<()> {
        let client = self.client().await?;
        github_api::validate_token(&client).await
    }

    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>> {
        let client = self.client().await?;
        github_api::list_repos(&client).await
    }

    async fn list_files(&self, branch: &str) -> Result<Vec<String>> {
        let client = self.client().await?;
        github_api::list_files(&client, &self.owner, &self.repo, branch).await
    }

    async fn read_file(&self, branch: &str, path: &str) -> Result<RemoteDocument> {
        let client = self.client().await?;
        github_api::read_file(&client, &self.owner, &self.repo, branch, path).await
    }

    async fn write_files(
        &self,
        branch: &str,
        inputs: Vec<WriteFileInput>,
    ) -> Result<Vec<WriteFileResult>> {
        let client = self.client().await?;
        github_api::create_tree_and_commit(
            &client,
            &self.owner,
            &self.repo,
            branch,
            inputs,
        )
        .await
    }

    async fn delete_file(&self, branch: &str, path: &str, message: &str) -> Result<()> {
        let client = self.client().await?;
        github_api::delete_file(&client, &self.owner, &self.repo, branch, path, message).await
    }

    async fn get_head(&self, branch: &str) -> Result<String> {
        let client = self.client().await?;
        github_api::get_head(&client, &self.owner, &self.repo, branch).await
    }
}
