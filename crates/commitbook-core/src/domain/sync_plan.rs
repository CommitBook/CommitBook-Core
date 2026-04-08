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
    pub workspace_id: String,
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

// --- Sync Jobs ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncJobType {
    InitialPull,
    SyncNow,
    ScheduledPull,
    ScheduledPush,
    ReindexWorkspace,
    ResolveConflict,
}

impl SyncJobType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InitialPull => "initial_pull",
            Self::SyncNow => "sync_now",
            Self::ScheduledPull => "scheduled_pull",
            Self::ScheduledPush => "scheduled_push",
            Self::ReindexWorkspace => "reindex_workspace",
            Self::ResolveConflict => "resolve_conflict",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "initial_pull" => Some(Self::InitialPull),
            "sync_now" => Some(Self::SyncNow),
            "scheduled_pull" => Some(Self::ScheduledPull),
            "scheduled_push" => Some(Self::ScheduledPush),
            "reindex_workspace" => Some(Self::ReindexWorkspace),
            "resolve_conflict" => Some(Self::ResolveConflict),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SyncJobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJob {
    pub id: String,
    pub workspace_id: String,
    pub job_type: SyncJobType,
    pub status: SyncJobStatus,
    pub retry_count: u32,
    pub max_retries: u32,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

impl SyncJob {
    pub fn new_id() -> String {
        format!("sj_{}", nanoid::nanoid!(12))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCheckpoint {
    pub workspace_id: String,
    pub remote_head: String,
    pub last_completed_at: String,
}
