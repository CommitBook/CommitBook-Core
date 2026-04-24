use anyhow::Result;
use std::path::Path;

use crate::domain::sync_plan::{SyncMode, SyncPlan};
use crate::domain::transport::{RemoteTransport, WriteFileInput};
use crate::git::{self, GitRepo};
use crate::logger::FileLogger;
use crate::markdown::parser::parse_document;
use crate::markdown::reassemble::reassemble;
use crate::merge::engine::merge_document;
use crate::state::sync_state::SyncState;

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
    logger: &FileLogger,
    commit_message: Option<String>,
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
            pull_documents(plan, repo_root, branch, transport, &mut result, logger).await?;
        }
        SyncMode::PushOnly => {
            push_documents(
                plan,
                repo_root,
                branch,
                transport,
                &commit_message,
                &mut result,
                logger,
            )
            .await?;
        }
        SyncMode::PullThenPush => {
            pull_documents(plan, repo_root, branch, transport, &mut result, logger).await?;
            push_documents(
                plan,
                repo_root,
                branch,
                transport,
                &commit_message,
                &mut result,
                logger,
            )
            .await?;
        }
    }

    // Update checkpoint only if no errors occurred. The new remote_head SHA
    // is the base SHA for the next sync — there is no separate base cache.
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
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    result: &mut SyncResult,
    logger: &FileLogger,
) -> Result<()> {
    let repo = GitRepo::open(repo_root).ok();

    for doc_plan in &plan.documents {
        if !doc_plan.requires_download {
            continue;
        }

        // Download the remote version.
        let remote_doc = match transport.read_file(branch, &doc_plan.path).await {
            Ok(doc) => doc,
            Err(e) => {
                let msg = format!("Failed to download {}: {e}", doc_plan.path);
                let _ = logger.error(&msg);
                result.errors.push(msg);
                continue;
            }
        };

        if doc_plan.requires_merge {
            // Three-way merge: base (from git) vs local (working tree) vs remote.
            let base_content = repo
                .as_ref()
                .and_then(|r| git::base::read(r, &plan.base_revision, &doc_plan.path))
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
            if local_content == remote_doc.content {
                // Nothing to do — working tree already matches remote.
                continue;
            } else if local_content == base_content {
                // Only remote changed — take remote content verbatim.
                let file_path = repo_root.join(&doc_plan.path);
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&file_path, &remote_doc.content)?;
                let _ = logger.info(&format!("Pulled {}", doc_plan.path));
                result.pulled += 1;
            } else if remote_doc.content == base_content {
                // Only local changed — keep local as-is.
                continue;
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
                result.pulled += 1;

                if !merge_result.conflicts.is_empty() {
                    result.conflicts += merge_result.conflicts.len() as u32;
                    let _ = logger.warn(&format!(
                        "{} conflict(s) in {}",
                        merge_result.conflicts.len(),
                        doc_plan.path
                    ));
                    log::warn!(
                        "{} conflict(s) in {}",
                        merge_result.conflicts.len(),
                        doc_plan.path
                    );
                } else {
                    let _ = logger.info(&format!("Merged {}", doc_plan.path));
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
                let _ = logger.info(&format!("Pulled {}", doc_plan.path));
                result.pulled += 1;
            }
        }
    }

    // Handle remote deletions — just remove from working tree. The base SHA
    // advances on successful sync so the deleted path disappears naturally.
    for doc_plan in &plan.documents {
        if doc_plan.requires_delete {
            let file_path = repo_root.join(&doc_plan.path);
            if file_path.exists() {
                std::fs::remove_file(&file_path)?;
            }
        }
    }

    Ok(())
}

async fn push_documents(
    plan: &SyncPlan,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    commit_message: &Option<String>,
    result: &mut SyncResult,
    logger: &FileLogger,
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

        let msg = commit_message
            .clone()
            .unwrap_or_else(|| format!("Update {} via CommitBook", doc_plan.path));

        to_push.push(WriteFileInput {
            path: doc_plan.path.clone(),
            content,
            message: msg,
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
            let _ = logger.info(&format!("Pushed {} file(s)", write_results.len()));
            Ok(true)
        }
        Err(e) => {
            let msg = format!("Push failed: {e}");
            let _ = logger.error(&msg);
            result.errors.push(msg);
            Ok(false)
        }
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
