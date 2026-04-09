use anyhow::Result;
use rusqlite::Connection;

use crate::domain::sync_plan::{
    DocumentState, PlannedDocumentSync, SyncCheckpoint, SyncMode, SyncPlan,
};
use crate::domain::transport::RemoteTransport;
use crate::domain::workspace::Workspace;
use crate::storage::{document_repo, sync_repo};

/// Create a sync plan by comparing local state against the remote.
pub async fn create_sync_plan(
    workspace: &Workspace,
    transport: &dyn RemoteTransport,
    conn: &Connection,
) -> Result<SyncPlan> {
    let checkpoint = sync_repo::get_checkpoint(conn, &workspace.id)?;
    let remote_head = transport.get_head(&workspace.branch).await?;
    let local_dirty = document_repo::list_dirty(conn, &workspace.id)?;
    let local_docs = document_repo::list_by_workspace(conn, &workspace.id)?;

    let remote_changed = match &checkpoint {
        Some(cp) => cp.remote_head != remote_head,
        None => true, // No checkpoint = first sync, always pull.
    };
    let has_dirty = !local_dirty.is_empty();

    // Determine sync mode.
    let mode = match (remote_changed, has_dirty) {
        (false, false) => SyncMode::Noop,
        (true, false) => SyncMode::PullOnly,
        (false, true) => SyncMode::PushOnly,
        (true, true) => SyncMode::PullThenPush,
    };

    if mode == SyncMode::Noop {
        return Ok(SyncPlan {
            workspace_id: workspace.id.clone(),
            mode,
            documents: Vec::new(),
        });
    }

    // List remote files to compare.
    let remote_files = if remote_changed {
        transport.list_files(&workspace.branch).await?
    } else {
        Vec::new()
    };

    let mut planned_docs = Vec::new();

    // Build a set of local document paths.
    let local_paths: std::collections::HashSet<String> =
        local_docs.iter().map(|d| d.path.clone()).collect();
    let remote_paths: std::collections::HashSet<String> =
        remote_files.iter().cloned().collect();

    // For each remote file, determine what action is needed.
    for remote_path in &remote_files {
        let local_doc = local_docs.iter().find(|d| &d.path == remote_path);

        match local_doc {
            Some(doc) if doc.dirty => {
                // Both local dirty and remote exists → needs merge.
                planned_docs.push(PlannedDocumentSync {
                    path: remote_path.clone(),
                    local_state: DocumentState::Modified,
                    remote_state: DocumentState::Modified,
                    requires_merge: true,
                    requires_conflict: false, // merge will determine
                    requires_upload: true,
                    requires_download: true,
                    requires_delete: false,
                });
            }
            Some(_doc) => {
                // Local exists but not dirty → download if changed.
                planned_docs.push(PlannedDocumentSync {
                    path: remote_path.clone(),
                    local_state: DocumentState::Unchanged,
                    remote_state: DocumentState::Modified,
                    requires_merge: false,
                    requires_conflict: false,
                    requires_upload: false,
                    requires_download: true,
                    requires_delete: false,
                });
            }
            None => {
                // New file on remote.
                planned_docs.push(PlannedDocumentSync {
                    path: remote_path.clone(),
                    local_state: DocumentState::Unknown,
                    remote_state: DocumentState::Added,
                    requires_merge: false,
                    requires_conflict: false,
                    requires_upload: false,
                    requires_download: true,
                    requires_delete: false,
                });
            }
        }
    }

    // For dirty local files not on remote.
    for dirty_doc in &local_dirty {
        if !remote_paths.contains(&dirty_doc.path) {
            planned_docs.push(PlannedDocumentSync {
                path: dirty_doc.path.clone(),
                local_state: DocumentState::Modified,
                remote_state: DocumentState::Unknown,
                requires_merge: false,
                requires_conflict: false,
                requires_upload: true,
                requires_download: false,
                requires_delete: false,
            });
        }
    }

    // For local files deleted on remote.
    for local_doc in &local_docs {
        if !remote_paths.contains(&local_doc.path) && !local_doc.deleted && remote_changed {
            // Only mark for deletion if we had a previous checkpoint
            // (otherwise we don't know if the file was ever on remote).
            if checkpoint.is_some() {
                planned_docs.push(PlannedDocumentSync {
                    path: local_doc.path.clone(),
                    local_state: DocumentState::Unchanged,
                    remote_state: DocumentState::Deleted,
                    requires_merge: false,
                    requires_conflict: false,
                    requires_upload: false,
                    requires_download: false,
                    requires_delete: true,
                });
            }
        }
    }

    Ok(SyncPlan {
        workspace_id: workspace.id.clone(),
        mode,
        documents: planned_docs,
    })
}

#[cfg(test)]
#[path = "planner_tests.rs"]
mod tests;
