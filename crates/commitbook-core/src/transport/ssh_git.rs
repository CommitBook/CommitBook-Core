use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Command;

use crate::domain::transport::{
    RemoteDocument, RemoteTransport, RepoDescriptor, WriteFileInput, WriteFileResult,
};

/// SSH/git transport — hybrid approach using git2 for reads and system git for
/// SSH push/pull (to leverage the user's SSH agent and config).
pub struct SshGitTransport {
    /// Local clone path where CommitBook maintains a working copy.
    local_clone_path: PathBuf,
    remote_url: String,
    branch: String,
}

impl SshGitTransport {
    pub fn new(local_clone_path: PathBuf, remote_url: String, branch: String) -> Self {
        Self {
            local_clone_path,
            remote_url,
            branch,
        }
    }

    /// Ensure the local clone exists. If not, clone it.
    fn ensure_clone(&self) -> Result<()> {
        if self.local_clone_path.join(".git").exists() {
            return Ok(());
        }

        let output = Command::new("git")
            .args([
                "clone",
                "--branch",
                &self.branch,
                "--single-branch",
                &self.remote_url,
                &self.local_clone_path.to_string_lossy(),
            ])
            .output()
            .context("Failed to run git clone")?;

        if !output.status.success() {
            bail!(
                "git clone failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Fetch latest from remote using system git (for SSH agent support).
    fn fetch(&self) -> Result<()> {
        let output = Command::new("git")
            .current_dir(&self.local_clone_path)
            .args(["fetch", "origin", &self.branch])
            .output()
            .context("Failed to run git fetch")?;

        if !output.status.success() {
            bail!(
                "git fetch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Push to remote using system git.
    fn push(&self) -> Result<()> {
        let output = Command::new("git")
            .current_dir(&self.local_clone_path)
            .args(["push", "origin", &self.branch])
            .output()
            .context("Failed to run git push")?;

        if !output.status.success() {
            bail!(
                "git push failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}

#[async_trait]
impl RemoteTransport for SshGitTransport {
    async fn validate(&self) -> Result<()> {
        self.ensure_clone()?;
        self.fetch()?;
        Ok(())
    }

    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>> {
        // SSH transport manages a single repo.
        let name = self
            .remote_url
            .rsplit('/')
            .next()
            .unwrap_or("repo")
            .trim_end_matches(".git")
            .to_string();
        Ok(vec![RepoDescriptor {
            id: self.remote_url.clone(),
            provider: "ssh".to_string(),
            owner: String::new(),
            name,
            default_branch: self.branch.clone(),
            private: false,
        }])
    }

    async fn list_files(&self, _branch: &str) -> Result<Vec<String>> {
        self.ensure_clone()?;
        self.fetch()?;

        // Use the local clone's working tree to list files.
        let local = super::local_repo::LocalRepoTransport::new(
            self.local_clone_path.clone(),
            self.branch.clone(),
        );
        local.list_files(_branch).await
    }

    async fn read_file(&self, _branch: &str, path: &str) -> Result<RemoteDocument> {
        self.ensure_clone()?;

        let local = super::local_repo::LocalRepoTransport::new(
            self.local_clone_path.clone(),
            self.branch.clone(),
        );
        local.read_file(_branch, path).await
    }

    async fn write_files(
        &self,
        _branch: &str,
        inputs: Vec<WriteFileInput>,
    ) -> Result<Vec<WriteFileResult>> {
        self.ensure_clone()?;

        // Write and commit locally.
        let local = super::local_repo::LocalRepoTransport::new(
            self.local_clone_path.clone(),
            self.branch.clone(),
        );
        let results = local.write_files(_branch, inputs).await?;

        // Push to remote via system git (SSH agent).
        self.push()?;

        Ok(results)
    }

    async fn delete_file(&self, _branch: &str, path: &str, message: &str) -> Result<()> {
        self.ensure_clone()?;

        let local = super::local_repo::LocalRepoTransport::new(
            self.local_clone_path.clone(),
            self.branch.clone(),
        );
        local.delete_file(_branch, path, message).await?;

        self.push()?;
        Ok(())
    }

    async fn get_head(&self, _branch: &str) -> Result<String> {
        self.ensure_clone()?;
        self.fetch()?;

        let local = super::local_repo::LocalRepoTransport::new(
            self.local_clone_path.clone(),
            self.branch.clone(),
        );
        local.get_head(_branch).await
    }
}
