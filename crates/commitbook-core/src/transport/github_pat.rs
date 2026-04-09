use anyhow::Result;
use async_trait::async_trait;

use crate::domain::transport::{
    RemoteDocument, RemoteTransport, RepoDescriptor, WriteFileInput, WriteFileResult,
};

use super::github_api;

/// GitHub PAT transport — direct API access with a personal access token.
/// No backend dependency.
pub struct GithubPatTransport {
    token: String,
    owner: String,
    repo: String,
    client: reqwest::Client,
}

impl GithubPatTransport {
    pub fn new(token: String, owner: String, repo: String) -> Result<Self> {
        let client = github_api::github_client(&token)?;
        Ok(Self {
            token,
            owner,
            repo,
            client,
        })
    }
}

#[async_trait]
impl RemoteTransport for GithubPatTransport {
    async fn validate(&self) -> Result<()> {
        github_api::validate_token(&self.client).await
    }

    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>> {
        github_api::list_repos(&self.client).await
    }

    async fn list_files(&self, branch: &str) -> Result<Vec<String>> {
        github_api::list_files(&self.client, &self.owner, &self.repo, branch).await
    }

    async fn read_file(&self, branch: &str, path: &str) -> Result<RemoteDocument> {
        github_api::read_file(&self.client, &self.owner, &self.repo, branch, path).await
    }

    async fn write_files(
        &self,
        branch: &str,
        inputs: Vec<WriteFileInput>,
    ) -> Result<Vec<WriteFileResult>> {
        github_api::create_tree_and_commit(
            &self.client,
            &self.owner,
            &self.repo,
            branch,
            inputs,
        )
        .await
    }

    async fn delete_file(&self, branch: &str, path: &str, message: &str) -> Result<()> {
        github_api::delete_file(&self.client, &self.owner, &self.repo, branch, path, message)
            .await
    }

    async fn get_head(&self, branch: &str) -> Result<String> {
        github_api::get_head(&self.client, &self.owner, &self.repo, branch).await
    }
}
