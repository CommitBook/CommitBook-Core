use anyhow::Result;
use async_trait::async_trait;

/// Describes a remote repository.
#[derive(Debug, Clone)]
pub struct RepoDescriptor {
    pub id: String,
    pub provider: String,
    pub owner: String,
    pub name: String,
    pub default_branch: String,
    pub private: bool,
}

/// A document fetched from a remote source.
#[derive(Debug, Clone)]
pub struct RemoteDocument {
    pub path: String,
    pub content: String,
    /// Commit SHA or blob SHA identifying this version.
    pub revision: String,
}

/// Input for writing a file to a remote source.
#[derive(Debug, Clone)]
pub struct WriteFileInput {
    pub path: String,
    pub content: String,
    pub message: String,
    /// For optimistic concurrency — the revision this write is based on.
    pub base_revision: Option<String>,
}

/// Result of writing a file to a remote source.
#[derive(Debug, Clone)]
pub struct WriteFileResult {
    pub path: String,
    pub new_revision: String,
}

/// Abstraction over different remote Git hosting transports.
///
/// Implementations exist for GitHub App (API via backend tokens), GitHub PAT
/// (direct API), SSH/git (clone/fetch/push), and existing local repos.
#[async_trait]
pub trait RemoteTransport: Send + Sync {
    /// Validate that credentials are working.
    async fn validate(&self) -> Result<()>;

    /// List repositories accessible with current credentials.
    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>>;

    /// List file paths in the repo on the given branch.
    async fn list_files(&self, branch: &str) -> Result<Vec<String>>;

    /// Read a single file from the remote.
    async fn read_file(&self, branch: &str, path: &str) -> Result<RemoteDocument>;

    /// Write one or more files atomically (where supported).
    async fn write_files(
        &self,
        branch: &str,
        inputs: Vec<WriteFileInput>,
    ) -> Result<Vec<WriteFileResult>>;

    /// Delete a file on the remote.
    async fn delete_file(&self, branch: &str, path: &str, message: &str) -> Result<()>;

    /// Get the current HEAD commit SHA for a branch.
    async fn get_head(&self, branch: &str) -> Result<String>;
}
