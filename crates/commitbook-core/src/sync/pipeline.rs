use anyhow::Result;
use std::path::Path;

use crate::domain::sync_plan::{SyncMode, SyncPlan};
use crate::domain::transport::{RemoteTransport, WriteFileInput};
use crate::git::{self, GitRepo};
use crate::platform::Logger;
use crate::markdown::parser::parse_document;
use crate::markdown::reassemble::reassemble;
use crate::merge::engine::merge_document;
use crate::state::sync_state::SyncState;

/// Result of executing a sync plan.
///
/// Push counts are split by operation kind so the CLI can surface an
/// "Added 1, Modified 2, Deleted 1" summary. `pushed()` collapses them back
/// to a single total for callers that don't care about the breakdown.
#[derive(Debug, Default)]
pub struct SyncResult {
    pub pulled: u32,
    pub pushed_added: u32,
    pub pushed_modified: u32,
    pub pushed_deleted: u32,
    pub conflicts: u32,
    pub errors: Vec<String>,
}

impl SyncResult {
    /// Total files pushed across add / modify / delete categories.
    pub fn pushed(&self) -> u32 {
        self.pushed_added + self.pushed_modified + self.pushed_deleted
    }
}

/// Execute a sync plan.
pub async fn execute_sync(
    plan: &SyncPlan,
    commitbook_dir: &Path,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    logger: &dyn Logger,
    commit_message: Option<String>,
) -> Result<SyncResult> {
    let mut result = SyncResult::default();

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
    logger: &dyn Logger,
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
    logger: &dyn Logger,
) -> Result<bool> {
    let push_ok = push_uploads(
        plan, repo_root, branch, transport, commit_message, result, logger,
    )
    .await?;
    push_remote_deletes(plan, branch, transport, commit_message, result, logger).await?;
    Ok(push_ok)
}

async fn push_uploads(
    plan: &SyncPlan,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    commit_message: &Option<String>,
    result: &mut SyncResult,
    logger: &dyn Logger,
) -> Result<bool> {
    // Collect files to push.
    let mut to_push: Vec<WriteFileInput> = Vec::new();
    // Side table of the uploaded paths' "was this new vs existing at base?"
    // categorization, in the same order as `to_push`. Used to split the
    // result counters once write_files returns successfully.
    let mut is_new_at_base: Vec<bool> = Vec::new();

    let repo = GitRepo::open(repo_root).ok();

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

        // "Added" vs "Modified" from the user's POV = whether the file
        // existed at the base SHA. We already have show_file_at_ref/base::read
        // for exactly this.
        let existed_at_base = repo
            .as_ref()
            .and_then(|r| git::base::read(r, &plan.base_revision, &doc_plan.path))
            .is_some();
        is_new_at_base.push(!existed_at_base);

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
            let pushed_paths: std::collections::HashSet<&str> =
                write_results.iter().map(|r| r.path.as_str()).collect();

            // Walk the parallel categorization we built above, counting only
            // the paths that actually ended up in the commit (write_results
            // filters out stable-content inputs).
            let mut added = 0u32;
            let mut modified = 0u32;
            for (input, is_new) in to_push_paths_with_flags(plan, &is_new_at_base)
                .into_iter()
                .filter(|(path, _)| pushed_paths.contains(path.as_str()))
            {
                if is_new {
                    added += 1;
                } else {
                    modified += 1;
                }
                let _ = input;
            }
            result.pushed_added += added;
            result.pushed_modified += modified;

            let names: Vec<String> = write_results
                .iter()
                .map(|r| r.path.clone())
                .collect();
            log_paths(logger, "Pushed", &names);
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

/// Rebuild the (path, is_new_at_base) pairs produced by `push_uploads` so we
/// can correlate them with the set of paths the transport actually committed.
fn to_push_paths_with_flags(
    plan: &SyncPlan,
    is_new_at_base: &[bool],
) -> Vec<(String, bool)> {
    plan.documents
        .iter()
        .filter(|d| d.requires_upload)
        .zip(is_new_at_base.iter().copied())
        .map(|(doc, is_new)| (doc.path.clone(), is_new))
        .collect()
}

async fn push_remote_deletes(
    plan: &SyncPlan,
    branch: &str,
    transport: &dyn RemoteTransport,
    commit_message: &Option<String>,
    result: &mut SyncResult,
    logger: &dyn Logger,
) -> Result<()> {
    let to_delete: Vec<&crate::domain::sync_plan::PlannedDocumentSync> = plan
        .documents
        .iter()
        .filter(|d| d.requires_remote_delete)
        .collect();

    if to_delete.is_empty() {
        return Ok(());
    }

    let msg = commit_message
        .clone()
        .unwrap_or_else(|| format!("Delete {} file(s) via CommitBook", to_delete.len()));

    let mut deleted_paths: Vec<String> = Vec::new();
    for doc in to_delete {
        match transport.delete_file(branch, &doc.path, &msg).await {
            Ok(()) => {
                result.pushed_deleted += 1;
                deleted_paths.push(doc.path.clone());
            }
            Err(e) => {
                let emsg = format!("Failed to delete {}: {e}", doc.path);
                let _ = logger.error(&emsg);
                result.errors.push(emsg);
            }
        }
    }
    log_paths(logger, "Deleted", &deleted_paths);

    Ok(())
}

/// Emit `"{verb}: a.md, b.md, c.md"` — truncated to the first 5 with
/// `"(and N more)"` trailer. Silent when the list is empty.
fn log_paths(logger: &dyn Logger, verb: &str, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    if paths.len() <= 5 {
        let _ = logger.info(&format!("{verb}: {}", paths.join(", ")));
    } else {
        let _ = logger.info(&format!(
            "{verb}: {} (and {} more)",
            paths[..5].join(", "),
            paths.len() - 5
        ));
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
