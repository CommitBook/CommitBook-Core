use anyhow::Result;
use std::path::Path;

use crate::domain::sync_plan::{SyncMode, SyncPlan};
use crate::domain::transport::{RemoteTransport, WriteFileInput};
use crate::markdown::parser::parse_document;
use crate::markdown::reassemble::reassemble;
use crate::merge::engine::merge_document;
use crate::state::{base, sync_state::SyncState};

/// Result of executing a sync plan.
#[derive(Debug)]
pub struct SyncResult {
    pub pulled: u32,
    pub pushed: u32,
    pub conflicts: u32,
    pub errors: Vec<String>,
}

/// Execute a sync plan.
pub async fn execute_sync(
    plan: &SyncPlan,
    commitbook_dir: &Path,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
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
            pull_documents(plan, commitbook_dir, repo_root, branch, transport, &mut result)
                .await?;
        }
        SyncMode::PushOnly => {
            push_documents(plan, repo_root, branch, transport, &mut result).await?;
        }
        SyncMode::PullThenPush => {
            pull_documents(plan, commitbook_dir, repo_root, branch, transport, &mut result)
                .await?;
            push_documents(plan, repo_root, branch, transport, &mut result).await?;
        }
    }

    // Update checkpoint after successful sync.
    if let Ok(head) = transport.get_head(branch).await {
        let state = SyncState {
            remote_head: Some(head),
            last_sync_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        state.save(commitbook_dir)?;
    }

    Ok(result)
}

async fn pull_documents(
    plan: &SyncPlan,
    commitbook_dir: &Path,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    result: &mut SyncResult,
) -> Result<()> {
    for doc_plan in &plan.documents {
        if !doc_plan.requires_download {
            continue;
        }

        // Download the remote version.
        let remote_doc = match transport.read_file(branch, &doc_plan.path).await {
            Ok(doc) => doc,
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to download {}: {e}", doc_plan.path));
                continue;
            }
        };

        if doc_plan.requires_merge {
            // Three-way merge: base vs local (working tree) vs remote.
            let base_content = base::read(commitbook_dir, &doc_plan.path)?
                .unwrap_or_default();
            let local_content = {
                let local_path = repo_root.join(&doc_plan.path);
                if local_path.exists() {
                    std::fs::read_to_string(&local_path)?
                } else {
                    String::new()
                }
            };

            let base_tree = parse_document(&base_content);
            let local_tree = parse_document(&local_content);
            let remote_tree = parse_document(&remote_doc.content);

            let merge_result = merge_document(&base_tree, &local_tree, &remote_tree);
            let merged_content = reassemble(&merge_result.merged_tree);

            // Write merged content to working tree.
            let file_path = repo_root.join(&doc_plan.path);
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file_path, &merged_content)?;

            // Update base version.
            base::write(commitbook_dir, &doc_plan.path, &merged_content)?;

            if !merge_result.conflicts.is_empty() {
                result.conflicts += merge_result.conflicts.len() as u32;
                log::warn!(
                    "{} conflict(s) in {}",
                    merge_result.conflicts.len(),
                    doc_plan.path
                );
            }
        } else {
            // No merge needed — write remote content directly.
            let file_path = repo_root.join(&doc_plan.path);
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file_path, &remote_doc.content)?;

            // Save as base version for future merges.
            base::write(commitbook_dir, &doc_plan.path, &remote_doc.content)?;
        }

        result.pulled += 1;
    }

    // Handle remote deletions.
    for doc_plan in &plan.documents {
        if doc_plan.requires_delete {
            // Remove from working tree.
            let file_path = repo_root.join(&doc_plan.path);
            if file_path.exists() {
                std::fs::remove_file(&file_path)?;
            }
            // Remove base version.
            base::delete(commitbook_dir, &doc_plan.path)?;
        }
    }

    Ok(())
}

async fn push_documents(
    plan: &SyncPlan,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    result: &mut SyncResult,
) -> Result<()> {
    // Collect files to push.
    let mut to_push: Vec<WriteFileInput> = Vec::new();

    for doc_plan in &plan.documents {
        if !doc_plan.requires_upload {
            continue;
        }

        // Read current content from working tree.
        let file_path = repo_root.join(&doc_plan.path);
        let content = if file_path.exists() {
            std::fs::read_to_string(&file_path)?
        } else {
            continue;
        };

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
    match transport.write_files(branch, to_push).await {
        Ok(write_results) => {
            result.pushed += write_results.len() as u32;
        }
        Err(e) => {
            result.errors.push(format!("Push failed: {e}"));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
