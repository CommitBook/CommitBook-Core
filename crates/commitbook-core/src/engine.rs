use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::domain::conflict::ResolutionType;
use crate::domain::document::Document;
use crate::domain::transport::RemoteTransport;
use crate::domain::workspace::{LocalMode, Provider, Workspace, WorkspaceMode};
use crate::markdown::parser::parse_document;
use crate::storage::{conflict_repo, db, document_repo, workspace_repo};
use crate::sync::scheduler::sync_workspace;
use crate::transport::local_repo::LocalRepoTransport;

/// High-level engine facade — the single entry point for all platforms.
///
/// This is the API that Swift (via UniFFI), the TUI, the web dashboard,
/// and the CLI all call. It manages a SQLite database and provides
/// workspace, document, sync, and conflict operations.
pub struct CommitBookEngine {
    conn: Connection,
    /// Directory for sandbox workspace clones. Will be used when
    /// creating non-local workspaces (SSH, GitHub App, PAT).
    #[allow(dead_code)]
    workspaces_root: PathBuf,
}

// --- Public API types returned by the engine ---

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub provider: String,
    pub branch: String,
    pub auto_sync: bool,
    pub doc_count: usize,
    pub conflict_count: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentSummary {
    pub path: String,
    pub dirty: bool,
    pub has_conflicts: bool,
    pub checksum: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentContent {
    pub path: String,
    pub content: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncResultSummary {
    pub pulled: u32,
    pub pushed: u32,
    pub conflicts: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
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

impl CommitBookEngine {
    /// Create a new engine instance.
    ///
    /// - `db_path`: Path to the SQLite database file.
    /// - `workspaces_root`: Directory for sandbox workspace clones.
    pub fn new(db_path: &Path, workspaces_root: &Path) -> Result<Self> {
        std::fs::create_dir_all(workspaces_root)?;
        let conn = db::open_database(db_path)?;
        Ok(Self {
            conn,
            workspaces_root: workspaces_root.to_path_buf(),
        })
    }

    /// Create a new engine with an in-memory database (for testing).
    pub fn in_memory() -> Result<Self> {
        let conn = db::open_in_memory()?;
        Ok(Self {
            conn,
            workspaces_root: PathBuf::from("/tmp/commitbook-test"),
        })
    }

    // --- Workspace operations ---

    pub fn create_workspace(
        &self,
        name: &str,
        mode: &str,
        provider: &str,
        remote_url: Option<&str>,
        owner: Option<&str>,
        repo_name: Option<&str>,
        branch: &str,
        local_root: &str,
    ) -> Result<WorkspaceSummary> {
        let ws = Workspace {
            id: Workspace::new_id(),
            name: name.to_string(),
            mode: WorkspaceMode::from_str(mode)
                .context("Invalid workspace mode")?,
            provider: Provider::from_str(provider)
                .context("Invalid provider")?,
            remote_url: remote_url.map(|s| s.to_string()),
            owner: owner.map(|s| s.to_string()),
            repo_name: repo_name.map(|s| s.to_string()),
            branch: branch.to_string(),
            local_mode: LocalMode::Folder,
            local_root: local_root.to_string(),
            merge_mode: "section_aware".to_string(),
            sync_interval_seconds: 300,
            auto_sync: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        workspace_repo::insert(&self.conn, &ws)?;
        self.workspace_to_summary(&ws)
    }

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceSummary>> {
        let workspaces = workspace_repo::list(&self.conn)?;
        workspaces
            .iter()
            .map(|ws| self.workspace_to_summary(ws))
            .collect()
    }

    pub fn get_workspace(&self, id: &str) -> Result<WorkspaceSummary> {
        let ws = workspace_repo::get(&self.conn, id)?
            .with_context(|| format!("Workspace '{id}' not found"))?;
        self.workspace_to_summary(&ws)
    }

    pub fn delete_workspace(&self, id: &str) -> Result<()> {
        workspace_repo::delete(&self.conn, id)
    }

    // --- Document operations ---

    pub fn list_documents(&self, workspace_id: &str) -> Result<Vec<DocumentSummary>> {
        let docs = document_repo::list_by_workspace(&self.conn, workspace_id)?;
        let conflicts = conflict_repo::list_open(&self.conn, workspace_id)?;
        let conflict_paths: std::collections::HashSet<&str> =
            conflicts.iter().map(|c| c.path.as_str()).collect();

        Ok(docs
            .iter()
            .map(|d| DocumentSummary {
                path: d.path.clone(),
                dirty: d.dirty,
                has_conflicts: conflict_paths.contains(d.path.as_str()),
                checksum: d.checksum.clone(),
            })
            .collect())
    }

    pub fn read_document(&self, workspace_id: &str, path: &str) -> Result<DocumentContent> {
        let content = document_repo::get_version(
            &self.conn,
            workspace_id,
            path,
            "base",
        )?
        .unwrap_or_default();

        let doc = document_repo::get(&self.conn, workspace_id, path)?;
        let revision = doc.and_then(|d| d.remote_revision);

        Ok(DocumentContent {
            path: path.to_string(),
            content,
            revision,
        })
    }

    pub fn save_document(
        &self,
        workspace_id: &str,
        path: &str,
        content: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let checksum = Document::compute_checksum(content);

        document_repo::upsert(
            &self.conn,
            &Document {
                workspace_id: workspace_id.to_string(),
                path: path.to_string(),
                local_revision: None,
                remote_revision: None,
                checksum,
                dirty: true,
                deleted: false,
                updated_at: now.clone(),
            },
        )?;

        // Save the local version.
        document_repo::save_version(
            &self.conn,
            workspace_id,
            path,
            "local",
            content,
            "local",
        )?;

        // Update section index.
        let tree = parse_document(content);
        document_repo::upsert_sections(
            &self.conn,
            workspace_id,
            path,
            &tree.sections,
            &now,
        )?;

        Ok(())
    }

    // --- Sync operations ---

    pub async fn sync_workspace(&self, workspace_id: &str) -> Result<SyncResultSummary> {
        let ws = workspace_repo::get(&self.conn, workspace_id)?
            .with_context(|| format!("Workspace '{workspace_id}' not found"))?;

        let transport = self.create_transport(&ws)?;
        let result = sync_workspace(&ws, transport.as_ref(), &self.conn).await?;

        Ok(SyncResultSummary {
            pulled: result.pulled,
            pushed: result.pushed,
            conflicts: result.conflicts,
            errors: result.errors,
        })
    }

    // --- Conflict operations ---

    pub fn list_conflicts(&self, workspace_id: &str) -> Result<Vec<ConflictSummary>> {
        let conflicts = conflict_repo::list_open(&self.conn, workspace_id)?;
        Ok(conflicts
            .iter()
            .map(|c| ConflictSummary {
                id: c.id.clone(),
                path: c.path.clone(),
                section_path: c.section_path.clone(),
                conflict_type: c.conflict_type.as_str().to_string(),
                status: c.status.as_str().to_string(),
                local_content: c.local_content.clone(),
                remote_content: c.remote_content.clone(),
                opened_at: c.opened_at.clone(),
            })
            .collect())
    }

    pub fn resolve_conflict(
        &self,
        conflict_id: &str,
        resolution: &str,
        manual_content: Option<&str>,
    ) -> Result<()> {
        let resolution_type = ResolutionType::from_str(resolution)
            .context("Invalid resolution type")?;
        let now = chrono::Utc::now().to_rfc3339();

        let conflict = conflict_repo::get(&self.conn, conflict_id)?
            .with_context(|| format!("Conflict '{conflict_id}' not found"))?;

        // Apply resolution.
        let resolved_content = match resolution_type {
            ResolutionType::TakeLocal => conflict.local_content.clone(),
            ResolutionType::TakeRemote => conflict.remote_content.clone(),
            ResolutionType::KeepBoth => {
                crate::merge::conflict_builder::build_append_both(
                    &conflict.local_content,
                    &conflict.remote_content,
                    &now,
                )
            }
            ResolutionType::ManualEdit => {
                manual_content
                    .context("Manual resolution requires content")?
                    .to_string()
            }
        };

        // Update the document with the resolved content.
        self.save_document(
            &conflict.workspace_id,
            &conflict.path,
            &resolved_content,
        )?;

        // Mark conflict as resolved.
        conflict_repo::resolve(&self.conn, conflict_id, &resolution_type, &now)?;

        Ok(())
    }

    // --- Private helpers ---

    fn workspace_to_summary(&self, ws: &Workspace) -> Result<WorkspaceSummary> {
        let docs = document_repo::list_by_workspace(&self.conn, &ws.id)?;
        let conflicts = conflict_repo::list_open(&self.conn, &ws.id)?;

        Ok(WorkspaceSummary {
            id: ws.id.clone(),
            name: ws.name.clone(),
            mode: ws.mode.as_str().to_string(),
            provider: ws.provider.as_str().to_string(),
            branch: ws.branch.clone(),
            auto_sync: ws.auto_sync,
            doc_count: docs.len(),
            conflict_count: conflicts.len(),
        })
    }

    fn create_transport(&self, ws: &Workspace) -> Result<Box<dyn RemoteTransport>> {
        match ws.mode {
            WorkspaceMode::ExistingLocalRepo => Ok(Box::new(LocalRepoTransport::new(
                PathBuf::from(&ws.local_root),
                ws.branch.clone(),
            ))),
            WorkspaceMode::Ssh => {
                let remote_url = ws
                    .remote_url
                    .as_ref()
                    .context("SSH workspace missing remote URL")?;
                Ok(Box::new(crate::transport::ssh_git::SshGitTransport::new(
                    PathBuf::from(&ws.local_root),
                    remote_url.clone(),
                    ws.branch.clone(),
                )))
            }
            WorkspaceMode::Pat => {
                bail!("PAT transport requires token. Use login first.");
            }
            WorkspaceMode::GithubApp => {
                bail!("GitHub App transport requires backend session.");
            }
        }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
