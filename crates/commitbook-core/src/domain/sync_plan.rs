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
    pub requires_delete: bool,
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
