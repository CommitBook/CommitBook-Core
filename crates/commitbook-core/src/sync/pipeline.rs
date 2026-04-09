use anyhow::Result;
use rusqlite::Connection;

use crate::domain::conflict::{Conflict, ConflictStatus};
use crate::domain::document::Document;
use crate::domain::sync_plan::{SyncCheckpoint, SyncMode, SyncPlan};
use crate::domain::transport::{RemoteTransport, WriteFileInput};
use crate::domain::workspace::Workspace;
use crate::markdown::parser::parse_document;
use crate::markdown::reassemble::reassemble;
use crate::merge::engine::merge_document;
use crate::storage::{conflict_repo, document_repo, sync_repo};

/// Result of executing a sync plan.
#[derive(Debug)]
pub struct SyncResult {
    pub pulled: u32,
    pub pushed: u32,
    pub conflicts: u32,
    pub errors: Vec<String>,
}

/// Execute a sync plan against a workspace.
pub async fn execute_sync(
    plan: &SyncPlan,
    workspace: &Workspace,
    transport: &dyn RemoteTransport,
    conn: &Connection,
) -> Result<SyncResult> {
    let mut result = SyncResult {
        pulled: 0,
        pushed: 0,
        conflicts: 0,
        errors: Vec::new(),
    };

    match plan.mode {
        SyncMode::Noop => return Ok(result),
        SyncMode::PullOnly => {
            pull_documents(plan, workspace, transport, conn, &mut result).await?;
        }
        SyncMode::PushOnly => {
            push_documents(plan, workspace, transport, conn, &mut result).await?;
        }
        SyncMode::PullThenPush => {
            pull_documents(plan, workspace, transport, conn, &mut result).await?;
            push_documents(plan, workspace, transport, conn, &mut result).await?;
        }
    }

    // Update checkpoint after successful sync.
    if let Ok(head) = transport.get_head(&workspace.branch).await {
        sync_repo::upsert_checkpoint(
            conn,
            &SyncCheckpoint {
                workspace_id: workspace.id.clone(),
                remote_head: head,
                last_completed_at: chrono::Utc::now().to_rfc3339(),
            },
        )?;
    }

    Ok(result)
}

async fn pull_documents(
    plan: &SyncPlan,
    workspace: &Workspace,
    transport: &dyn RemoteTransport,
    conn: &Connection,
    result: &mut SyncResult,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    for doc_plan in &plan.documents {
        if !doc_plan.requires_download {
            continue;
        }

        // Download the remote version.
        let remote_doc = match transport
            .read_file(&workspace.branch, &doc_plan.path)
            .await
        {
            Ok(doc) => doc,
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to download {}: {e}", doc_plan.path));
                continue;
            }
        };

        // Check if we need to merge (local is dirty).
        if doc_plan.requires_merge {
            // Three-way merge: base (last known version) vs local vs remote.
            let base_content = document_repo::get_version(
                conn,
                &workspace.id,
                &doc_plan.path,
                "base",
            )?
            .unwrap_or_default();

            let local_doc = document_repo::get(conn, &workspace.id, &doc_plan.path)?;
            let local_content = if local_doc.is_some() {
                // Read current local content from the latest version.
                document_repo::get_version(
                    conn,
                    &workspace.id,
                    &doc_plan.path,
                    "local",
                )?
                .unwrap_or_default()
            } else {
                String::new()
            };

            let base_tree = parse_document(&base_content);
            let local_tree = parse_document(&local_content);
            let remote_tree = parse_document(&remote_doc.content);

            let merge_result = merge_document(&base_tree, &local_tree, &remote_tree);
            let merged_content = reassemble(&merge_result.merged_tree);

            // Save merged content as the new local version.
            let checksum = Document::compute_checksum(&merged_content);
            document_repo::upsert(
                conn,
                &Document {
                    workspace_id: workspace.id.clone(),
                    path: doc_plan.path.clone(),
                    local_revision: None,
                    remote_revision: Some(remote_doc.revision.clone()),
                    checksum,
                    dirty: !merge_result.conflicts.is_empty(),
                    deleted: false,
                    updated_at: now.clone(),
                },
            )?;

            // Save versions for future merges.
            document_repo::save_version(
                conn,
                &workspace.id,
                &doc_plan.path,
                "base",
                &merged_content,
                "merged",
            )?;

            // Create conflict records.
            for conflict in &merge_result.conflicts {
                let conflict_record = Conflict {
                    id: Conflict::new_id(),
                    workspace_id: workspace.id.clone(),
                    path: doc_plan.path.clone(),
                    section_path: conflict.section_path.clone(),
                    conflict_type: conflict.conflict_type.clone(),
                    base_content: conflict.base_content.clone(),
                    local_content: conflict.local_content.clone(),
                    remote_content: conflict.remote_content.clone(),
                    merged_preview: Some(merged_content.clone()),
                    status: ConflictStatus::Open,
                    resolution_type: None,
                    opened_at: now.clone(),
                    resolved_at: None,
                };
                conflict_repo::insert(conn, &conflict_record)?;
                result.conflicts += 1;
            }
        } else {
            // No merge needed — just store the remote version.
            let checksum = Document::compute_checksum(&remote_doc.content);
            document_repo::upsert(
                conn,
                &Document {
                    workspace_id: workspace.id.clone(),
                    path: doc_plan.path.clone(),
                    local_revision: None,
                    remote_revision: Some(remote_doc.revision.clone()),
                    checksum,
                    dirty: false,
                    deleted: false,
                    updated_at: now.clone(),
                },
            )?;

            // Save as base version for future merges.
            document_repo::save_version(
                conn,
                &workspace.id,
                &doc_plan.path,
                "base",
                &remote_doc.content,
                "remote",
            )?;
        }

        // Parse and store sections for fast comparison.
        let tree = parse_document(
            &document_repo::get_version(conn, &workspace.id, &doc_plan.path, "base")?
                .unwrap_or_default(),
        );
        document_repo::upsert_sections(
            conn,
            &workspace.id,
            &doc_plan.path,
            &tree.sections,
            &now,
        )?;

        result.pulled += 1;
    }

    // Handle remote deletions.
    for doc_plan in &plan.documents {
        if doc_plan.requires_delete {
            document_repo::delete(conn, &workspace.id, &doc_plan.path)?;
        }
    }

    Ok(())
}

async fn push_documents(
    plan: &SyncPlan,
    workspace: &Workspace,
    transport: &dyn RemoteTransport,
    conn: &Connection,
    result: &mut SyncResult,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // Collect files to push.
    let mut to_push: Vec<WriteFileInput> = Vec::new();

    for doc_plan in &plan.documents {
        if !doc_plan.requires_upload {
            continue;
        }

        // Read latest local content.
        let content = document_repo::get_version(
            conn,
            &workspace.id,
            &doc_plan.path,
            "local",
        )?
        .or_else(|| {
            document_repo::get_version(
                conn,
                &workspace.id,
                &doc_plan.path,
                "base",
            )
            .ok()
            .flatten()
        })
        .unwrap_or_default();

        to_push.push(WriteFileInput {
            path: doc_plan.path.clone(),
            content,
            message: format!("Update {} via CommitBook", doc_plan.path),
            base_revision: None,
        });
    }

    if to_push.is_empty() {
        return Ok(());
    }

    // Push all files atomically.
    match transport
        .write_files(&workspace.branch, to_push)
        .await
    {
        Ok(write_results) => {
            for wr in &write_results {
                // Clear dirty flag after successful push.
                if let Some(mut doc) =
                    document_repo::get(conn, &workspace.id, &wr.path)?
                {
                    doc.dirty = false;
                    doc.remote_revision = Some(wr.new_revision.clone());
                    doc.updated_at = now.clone();
                    document_repo::upsert(conn, &doc)?;
                }
                result.pushed += 1;
            }
        }
        Err(e) => {
            result
                .errors
                .push(format!("Push failed: {e}"));
        }
    }

    Ok(())
}
