use anyhow::Result;
use std::path::Path;

use crate::domain::sync_plan::{SyncMode, SyncPlan};
use crate::domain::transport::{RemoteTransport, WriteFileInput};
use crate::markdown::parser::parse_document;
use crate::markdown::reassemble::reassemble;
use crate::merge::engine::merge_document;
use crate::state::{base, sync_state::SyncState};

/// A base write deferred until push succeeds.
struct DeferredBaseWrite {
    path: String,
    content: String,
}

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
            let deferred = pull_documents(
                plan, commitbook_dir, repo_root, branch, transport, &mut result,
            )
            .await?;
            // No push phase — flush all deferred bases immediately.
            for d in &deferred {
                base::write(commitbook_dir, &d.path, &d.content)?;
            }
        }
        SyncMode::PushOnly => {
            push_documents(plan, repo_root, branch, transport, &mut result).await?;
        }
        SyncMode::PullThenPush => {
            let deferred = pull_documents(
                plan, commitbook_dir, repo_root, branch, transport, &mut result,
            )
            .await?;
            let push_ok =
                push_documents(plan, repo_root, branch, transport, &mut result).await?;
            if push_ok {
                for d in &deferred {
                    base::write(commitbook_dir, &d.path, &d.content)?;
                }
            }
        }
    }

    // Update checkpoint only if no errors occurred.
    if result.errors.is_empty() {
        if let Ok(head) = transport.get_head(branch).await {
            let state = SyncState {
                remote_head: Some(head),
                last_sync_at: Some(chrono::Utc::now().to_rfc3339()),
            };
            state.save(commitbook_dir)?;
        }
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
) -> Result<Vec<DeferredBaseWrite>> {
    let mut deferred_bases: Vec<DeferredBaseWrite> = Vec::new();

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

            // Fast-path: skip parse→merge→reassemble when content hasn't diverged.
            // Merge-path files always have requires_upload=true, so defer base writes.
            if local_content == remote_doc.content {
                // Local and remote are identical — just update base, no file write needed.
                deferred_bases.push(DeferredBaseWrite {
                    path: doc_plan.path.clone(),
                    content: local_content,
                });
            } else if local_content == base_content {
                // Only remote changed — take remote content verbatim (no reassemble).
                let file_path = repo_root.join(&doc_plan.path);
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&file_path, &remote_doc.content)?;
                deferred_bases.push(DeferredBaseWrite {
                    path: doc_plan.path.clone(),
                    content: remote_doc.content.clone(),
                });
                result.pulled += 1;
            } else if remote_doc.content == base_content {
                // Only local changed — keep local as-is, just update base.
                deferred_bases.push(DeferredBaseWrite {
                    path: doc_plan.path.clone(),
                    content: local_content,
                });
            } else {
                // Both sides changed — full three-way merge required.
                let base_tree = parse_document(&base_content);
                let local_tree = parse_document(&local_content);
                let remote_tree = parse_document(&remote_doc.content);

                let merge_result = merge_document(&base_tree, &local_tree, &remote_tree);
                let merged_content = reassemble(&merge_result.merged_tree);

                let file_path = repo_root.join(&doc_plan.path);
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&file_path, &merged_content)?;
                deferred_bases.push(DeferredBaseWrite {
                    path: doc_plan.path.clone(),
                    content: merged_content,
                });
                result.pulled += 1;

                if !merge_result.conflicts.is_empty() {
                    result.conflicts += merge_result.conflicts.len() as u32;
                    log::warn!(
                        "{} conflict(s) in {}",
                        merge_result.conflicts.len(),
                        doc_plan.path
                    );
                }
            }
        } else {
            // No merge needed — write remote content directly (only if different).
            let file_path = repo_root.join(&doc_plan.path);
            let local_content = if file_path.exists() {
                std::fs::read_to_string(&file_path).ok()
            } else {
                None
            };

            if local_content.as_deref() != Some(&remote_doc.content) {
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&file_path, &remote_doc.content)?;
                result.pulled += 1;
            }

            // Save as base version for future merges.
            if doc_plan.requires_upload {
                deferred_bases.push(DeferredBaseWrite {
                    path: doc_plan.path.clone(),
                    content: remote_doc.content.clone(),
                });
            } else {
                base::write(commitbook_dir, &doc_plan.path, &remote_doc.content)?;
            }
        }
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

    Ok(deferred_bases)
}

async fn push_documents(
    plan: &SyncPlan,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    result: &mut SyncResult,
) -> Result<bool> {
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
        return Ok(true);
    }

    // Push all files atomically.
    match transport.write_files(branch, to_push).await {
        Ok(write_results) => {
            result.pushed += write_results.len() as u32;
            Ok(true)
        }
        Err(e) => {
            result.errors.push(format!("Push failed: {e}"));
            Ok(false)
        }
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
