use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::domain::sync_plan::{
    DocumentState, PlannedDocumentSync, SyncMode, SyncPlan,
};
use crate::domain::transport::RemoteTransport;
use crate::git::GitRepo;
use crate::state::sync_state::SyncState;

/// Create a sync plan by comparing local state against the remote.
///
/// Local dirty detection uses `git status` (via `GitRepo::status_markdown`)
/// rather than walking the working tree — this catches modifications, untracked
/// additions, and deletions in one call. The planner's job is just to reconcile
/// those categories with the transport's view of the remote and emit
/// `PlannedDocumentSync` entries for the pipeline.
///
/// `tracked_patterns` is retained for API stability but no longer referenced:
/// status is git-driven, and `is_markdown_path` already filters inside
/// `status_markdown`.
pub async fn create_sync_plan(
    commitbook_dir: &Path,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    _tracked_patterns: &[String],
) -> Result<SyncPlan> {
    let state = SyncState::load(commitbook_dir)?;
    let remote_head = transport.get_head(branch).await?;

    let remote_changed = match &state.remote_head {
        Some(head) => *head != remote_head,
        None => true, // No checkpoint = first sync, always pull.
    };

    // Resolve the base SHA: checkpoint if we have one, else origin/<branch>
    // (first sync), else None (brand-new repo with no remote history).
    let repo = GitRepo::open(repo_root).ok();
    let base_sha: Option<String> = state
        .remote_head
        .clone()
        .or_else(|| {
            repo.as_ref()
                .and_then(|r| r.rev_parse(&format!("origin/{branch}")).ok())
        });

    // Git status is the single source of truth for local changes.
    let status = match repo.as_ref() {
        Some(r) => r.status_markdown()?,
        None => Default::default(),
    };

    let dirty_set: HashSet<String> = status
        .modified
        .iter()
        .chain(status.added.iter())
        .cloned()
        .collect();
    let deleted_set: HashSet<String> = status.deleted.iter().cloned().collect();

    let has_dirty = !dirty_set.is_empty() || !deleted_set.is_empty();

    let mode = match (remote_changed, has_dirty) {
        (false, false) => SyncMode::Noop,
        (true, false) => SyncMode::PullOnly,
        (false, true) => SyncMode::PushOnly,
        (true, true) => SyncMode::PullThenPush,
    };

    if mode == SyncMode::Noop {
        return Ok(SyncPlan {
            mode,
            documents: Vec::new(),
            base_revision: base_sha,
        });
    }

    // List remote files to compare.
    let remote_files = if remote_changed {
        transport.list_files(branch).await?
    } else {
        Vec::new()
    };
    let remote_paths: HashSet<String> = remote_files.iter().cloned().collect();

    // Markdown paths at the base commit — used to detect files that were removed
    // on remote since we last synced (so we can pull the delete into the working
    // tree).
    let base_files: Vec<String> = match (repo.as_ref(), base_sha.as_ref()) {
        (Some(r), Some(_)) => r
            .ls_tree_files(base_sha.as_ref().unwrap())?
            .into_iter()
            .filter(|p| is_markdown_path(p))
            .collect(),
        _ => Vec::new(),
    };

    let mut planned_docs = Vec::new();

    // For each remote file, decide the action. `dirty_set` is the set of local
    // paths with uncommitted content changes.
    for remote_path in &remote_files {
        let is_dirty = dirty_set.contains(remote_path);
        let exists_locally = !deleted_set.contains(remote_path)
            && (repo_root.join(remote_path).exists() || base_files.contains(remote_path));

        if is_dirty {
            // Both local dirty and remote exists → needs merge.
            planned_docs.push(PlannedDocumentSync {
                path: remote_path.clone(),
                local_state: DocumentState::Modified,
                remote_state: DocumentState::Modified,
                requires_merge: true,
                requires_conflict: false,
                requires_upload: true,
                requires_download: true,
                requires_delete: false,
                requires_remote_delete: false,
            });
        } else if exists_locally {
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
                requires_remote_delete: false,
            });
        } else if deleted_set.contains(remote_path) {
            // Local deleted something that still exists on remote → push the delete.
            planned_docs.push(PlannedDocumentSync {
                path: remote_path.clone(),
                local_state: DocumentState::Deleted,
                remote_state: DocumentState::Modified,
                requires_merge: false,
                requires_conflict: false,
                requires_upload: false,
                requires_download: false,
                requires_delete: false,
                requires_remote_delete: true,
            });
        } else {
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
                requires_remote_delete: false,
            });
        }
    }

    // Dirty local files not on remote → new file, push.
    for dirty_path in &dirty_set {
        if !remote_paths.contains(dirty_path) {
            planned_docs.push(PlannedDocumentSync {
                path: dirty_path.clone(),
                local_state: DocumentState::Modified,
                remote_state: DocumentState::Unknown,
                requires_merge: false,
                requires_conflict: false,
                requires_upload: true,
                requires_download: false,
                requires_delete: false,
                requires_remote_delete: false,
            });
        }
    }

    // Local deletions without a fresh remote fetch: still push the delete
    // optimistically (the transport may reject if remote moved; next cycle will
    // re-plan with the new state).
    if !remote_changed {
        for deleted_path in &deleted_set {
            planned_docs.push(PlannedDocumentSync {
                path: deleted_path.clone(),
                local_state: DocumentState::Deleted,
                remote_state: DocumentState::Unknown,
                requires_merge: false,
                requires_conflict: false,
                requires_upload: false,
                requires_download: false,
                requires_delete: false,
                requires_remote_delete: true,
            });
        }
    }

    // Base files gone from remote → pull the deletion into the working tree
    // (only when we had a previous checkpoint to trust).
    if state.remote_head.is_some() {
        for base_path in &base_files {
            if !remote_paths.contains(base_path)
                && !dirty_set.contains(base_path)
                && !deleted_set.contains(base_path)
            {
                planned_docs.push(PlannedDocumentSync {
                    path: base_path.clone(),
                    local_state: DocumentState::Unchanged,
                    remote_state: DocumentState::Deleted,
                    requires_merge: false,
                    requires_conflict: false,
                    requires_upload: false,
                    requires_download: false,
                    requires_delete: true,
                    requires_remote_delete: false,
                });
            }
        }
    }

    Ok(SyncPlan {
        mode,
        documents: planned_docs,
        base_revision: base_sha,
    })
}

fn is_markdown_path(p: &str) -> bool {
    if p.split('/').any(|seg| seg.starts_with('.')) {
        return false;
    }
    let lower = p.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

#[cfg(test)]
#[path = "planner_tests.rs"]
mod tests;
