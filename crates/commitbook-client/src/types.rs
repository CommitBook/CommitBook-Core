/// Per-sync conflict-handling mode. Apps pick per call.
#[derive(Debug, Clone, Copy)]
pub enum SyncMode {
    /// Auto-resolve via the configured AI resolver. Falls back to
    /// `manual_conflicts` only when the resolver fails.
    AiResolve,
    /// Don't auto-resolve. Leaves conflict markers in working tree
    /// and returns `manual_conflicts` count for the caller to handle
    /// via `list_conflicts` + `resolve_conflict`.
    Manual,
}

/// Input for `create_commitbook`. App provides the GitHub repo it
/// already picked from `discover_commitbooks`.
#[derive(Debug, Clone)]
pub struct CommitBookInput {
    pub name: String,
    /// Auth mode: "github_app" | "pat" | "ssh" | "existing_local_repo".
    pub mode: String,
    /// Provider: "github" | "gitlab" | "codeberg" | "generic_git".
    pub provider: String,
    pub owner: String,
    pub repo: String,
    pub branch: String,
}

/// Materialized view of a CommitBook clone on this device.
#[derive(Debug, Clone)]
pub struct CommitBookSummary {
    pub id: String,
    pub owner: String,
    pub repo: String,
    pub name: String,
    pub mode: String,
    pub provider: String,
    pub branch: String,
    pub auto_sync: bool,
    pub doc_count: u32,
    pub conflict_count: u32,
}

/// Result of `discover_commitbooks` — one entry per remote repo.
#[derive(Debug, Clone)]
pub struct DiscoveredCommitBook {
    pub owner: String,
    pub repo: String,
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

#[derive(Debug, Clone)]
pub struct ConflictSummary {
    pub id: String,
    pub path: String,
    pub section_path: Option<String>,
    pub conflict_type: String,
    pub status: String,
    pub local_content: String,
    pub remote_content: String,
    pub opened_at: String,
}

#[derive(Debug, Clone)]
pub struct ResolveConflictInput {
    pub commitbook_id: String,
    pub conflict_id: String,
    pub resolution_type: String,
    pub manual_content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub owner: String,
    pub name: String,
    pub default_branch: String,
    pub is_private: bool,
}
