//! Root client object exposed across the FFI boundary. All 12 engine
//! methods are stubbed at present — implementations land per-method
//! in subsequent commits.

use std::path::PathBuf;
use std::sync::Arc;

use crate::errors::{CommitBookError, Result};
use crate::types::*;

pub struct CommitBookEngineClient {
    pub(crate) db_path: PathBuf,
    pub(crate) workspaces_root: PathBuf,
}

impl CommitBookEngineClient {
    /// UniFFI-exposed constructor.
    pub fn new(db_path: String, workspaces_root: String) -> Result<Arc<Self>> {
        let db_path = PathBuf::from(db_path);
        let workspaces_root = PathBuf::from(workspaces_root);
        std::fs::create_dir_all(&workspaces_root).map_err(|e| {
            CommitBookError::database(format!(
                "Failed to create workspaces root {}: {e}",
                workspaces_root.display()
            ))
        })?;
        Ok(Arc::new(Self {
            db_path,
            workspaces_root,
        }))
    }

    pub async fn validate_pat(&self, _token: String) -> Result<Vec<RepoInfo>> {
        Err(CommitBookError::invalid_input(
            "validate_pat not yet implemented",
        ))
    }

    pub async fn discover_commitbooks(
        &self,
        _token: String,
    ) -> Result<Vec<DiscoveredCommitBook>> {
        Err(CommitBookError::invalid_input(
            "discover_commitbooks not yet implemented",
        ))
    }

    pub async fn create_commitbook(
        &self,
        _input: CommitBookInput,
        _token: String,
    ) -> Result<CommitBookSummary> {
        Err(CommitBookError::invalid_input(
            "create_commitbook not yet implemented",
        ))
    }

    pub fn list_commitbooks(&self) -> Result<Vec<CommitBookSummary>> {
        Err(CommitBookError::invalid_input(
            "list_commitbooks not yet implemented",
        ))
    }

    pub fn get_commitbook(&self, _commitbook_id: String) -> Result<CommitBookSummary> {
        Err(CommitBookError::invalid_input(
            "get_commitbook not yet implemented",
        ))
    }

    pub fn delete_commitbook(&self, _commitbook_id: String) -> Result<()> {
        Err(CommitBookError::invalid_input(
            "delete_commitbook not yet implemented",
        ))
    }

    pub fn list_documents(&self, _commitbook_id: String) -> Result<Vec<DocumentSummary>> {
        Err(CommitBookError::invalid_input(
            "list_documents not yet implemented",
        ))
    }

    pub fn read_document(
        &self,
        _commitbook_id: String,
        _path: String,
    ) -> Result<DocumentContent> {
        Err(CommitBookError::invalid_input(
            "read_document not yet implemented",
        ))
    }

    pub fn save_document(
        &self,
        _commitbook_id: String,
        _path: String,
        _content: String,
    ) -> Result<()> {
        Err(CommitBookError::invalid_input(
            "save_document not yet implemented",
        ))
    }

    pub async fn sync_commitbook(
        &self,
        _commitbook_id: String,
        _mode: SyncMode,
        _token: String,
    ) -> Result<SyncResultSummary> {
        Err(CommitBookError::invalid_input(
            "sync_commitbook not yet implemented",
        ))
    }

    pub fn list_conflicts(&self, _commitbook_id: String) -> Result<Vec<ConflictSummary>> {
        Err(CommitBookError::invalid_input(
            "list_conflicts not yet implemented",
        ))
    }

    pub fn resolve_conflict(&self, _input: ResolveConflictInput) -> Result<()> {
        Err(CommitBookError::invalid_input(
            "resolve_conflict not yet implemented",
        ))
    }
}
