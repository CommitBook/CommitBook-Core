use crate::errors::Result;

/// Per-sync conflict-handling mode. Apps pick per call.
#[derive(Debug, Clone, Copy)]
pub enum SyncMode {
    /// Auto-resolve via the configured AI resolver. Falls back to
    /// `manual_conflicts` only when the resolver fails.
    AiResolve,
    /// Don't auto-resolve, even when shared config selects both or review.
    /// Existing review proposals remain protected. Leaves conflict markers in working tree
    /// and returns `manual_conflicts` count for the caller to handle
    /// via `list_conflicts` + `resolve_conflict`.
    Manual,
}

/// Input for `init_commitbook`: a discovered or directly entered Git URL.
#[derive(Debug, Clone)]
pub struct CommitBookInput {
    pub name: String,
    /// Auth mode: "github_app" | "pat" | "ssh" | "existing_local_repo".
    pub mode: String,
    pub remote_url: String,
    pub branch: String,
    /// Name for this device in `.CommitBook/devices/`; `None` uses a default
    /// such as "iOS 7f3c".
    pub device_name: Option<String>,
}

/// Materialized view of a CommitBook clone on this device.
#[derive(Debug, Clone)]
pub struct CommitBookSummary {
    pub commitbook_local_id: String,
    pub remote_url: String,
    pub name: String,
    pub mode: String,
    pub provider: String,
    pub branch: String,
    pub auto_sync: bool,
    pub doc_count: u32,
    pub conflict_count: u32,
}

/// A managed clone whose config could not be loaded; see
/// `list_broken_commitbooks`.
#[derive(Debug, Clone)]
pub struct BrokenCommitBook {
    pub path: String,
    pub error: String,
}

/// Result of `discover_commitbooks`, one entry per remote repo.
#[derive(Debug, Clone)]
pub struct DiscoveredCommitBook {
    pub remote_url: String,
    pub name: String,
    pub default_branch: String,
    pub is_private: bool,
    pub has_dot_commitbook: bool,
    pub already_local: bool,
}

#[derive(Debug, Clone)]
pub struct DocumentSummary {
    pub path: String,
    pub dirty: bool,
    pub has_conflicts: bool,
    pub checksum: String,
}

#[derive(Debug, Clone)]
pub struct DocumentContent {
    pub path: String,
    pub content: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncResultSummary {
    pub committed: bool,
    pub pulled: u32,
    pub pushed: u32,
    pub conflicts_resolved: u32,
    pub manual_conflicts: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiConflictResolutionAction {
    WriteContent,
    DeleteFile,
}

#[derive(Debug, Clone)]
pub struct AiConflictRequest {
    pub commitbook_local_id: String,
    pub path: String,
    pub conflict_type: String,
    pub binary: bool,
    pub ancestor_content: Option<String>,
    pub local_content: Option<String>,
    pub remote_content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AiConflictResolution {
    pub action: AiConflictResolutionAction,
    pub content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AiConflictCallbackResult {
    pub resolution: Option<AiConflictResolution>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum GitCredentialKind {
    Default,
    Username,
    UserPassword,
    SshKey,
}

#[derive(Debug, Clone)]
pub struct GitCredentialRequest {
    pub remote_url: String,
    pub username_from_url: Option<String>,
    pub allow_default: bool,
    pub allow_username: bool,
    pub allow_user_password: bool,
    pub allow_ssh_key: bool,
}

// Intentionally not Debug: responses can contain secrets.
pub struct GitCredentialResponse {
    pub kind: GitCredentialKind,
    pub username: Option<String>,
    pub password: Option<String>,
    pub public_key: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    pub error_message: Option<String>,
}

pub trait GitCredentialCallback: Send + Sync {
    fn provide(&self, request: GitCredentialRequest) -> GitCredentialResponse;
}

/// Implemented by the embedding Swift/Kotlin application. `resolve` starts an
/// asynchronous host request and returns; the continuation completes it.
pub trait ConflictResolverCallback: Send + Sync {
    fn resolve(
        &self,
        request: AiConflictRequest,
        continuation: std::sync::Arc<ConflictResolutionContinuation>,
    );
}

pub struct ConflictResolutionContinuation {
    sender: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<AiConflictCallbackResult>>>,
}

impl ConflictResolutionContinuation {
    pub(crate) fn new(sender: tokio::sync::oneshot::Sender<AiConflictCallbackResult>) -> Self {
        Self {
            sender: std::sync::Mutex::new(Some(sender)),
        }
    }

    pub fn complete(&self, result: AiConflictCallbackResult) -> Result<()> {
        let sender = self
            .sender
            .lock()
            .map_err(|_| crate::errors::CommitBookError::storage("Resolver continuation poisoned"))?
            .take()
            .ok_or_else(|| {
                crate::errors::CommitBookError::invalid_input(
                    "Conflict resolver continuation was already completed",
                )
            })?;
        sender.send(result).map_err(|_| {
            crate::errors::CommitBookError::merge(
                "Conflict resolver continuation expired before completion",
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct ConflictSummary {
    pub id: String,
    pub path: String,
    pub section_path: Option<String>,
    pub conflict_type: String,
    pub status: String,
    pub binary: bool,
    pub ancestor_content: Option<String>,
    pub local_content: Option<String>,
    pub remote_content: Option<String>,
    pub opened_at: String,
    pub revision: Option<String>,
    pub proposal_content: Option<String>,
    pub proposal_version: Option<String>,
    pub proposal_stale: bool,
    pub proposal_rejected: bool,
}

#[derive(Debug, Clone)]
pub struct ResolveConflictInput {
    pub commitbook_local_id: String,
    pub conflict_id: String,
    pub resolution_type: String,
    pub manual_content: Option<String>,
    pub revision: Option<String>,
    pub proposal_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub remote_url: String,
    pub name: String,
    pub default_branch: String,
    pub is_private: bool,
}
