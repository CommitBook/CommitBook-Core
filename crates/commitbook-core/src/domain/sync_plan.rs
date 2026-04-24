use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    PullOnly,
    PushOnly,
    PullThenPush,
    Noop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub mode: SyncMode,
    pub documents: Vec<PlannedDocumentSync>,
    /// SHA to treat as the base version for dirty detection and 3-way merge.
    /// `None` on first sync when `origin/<branch>` doesn't resolve (brand-new repo).
    #[serde(default)]
    pub base_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedDocumentSync {
    pub path: String,
    pub local_state: DocumentState,
    pub remote_state: DocumentState,
    pub requires_merge: bool,
    pub requires_conflict: bool,
    pub requires_upload: bool,
    pub requires_download: bool,
    /// Remove the file from the local working tree (remote deleted it).
    pub requires_delete: bool,
    /// Push a deletion to the remote (local deleted it, remote still has it).
    #[serde(default)]
    pub requires_remote_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentState {
    Unchanged,
    Modified,
    Added,
    Deleted,
    Unknown,
}
