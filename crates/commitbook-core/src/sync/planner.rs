use anyhow::{Context, Result};
use std::path::Path;

use crate::domain::sync_plan::{
    DocumentState, PlannedDocumentSync, SyncMode, SyncPlan,
};
use crate::domain::transport::RemoteTransport;
use crate::git::{self, GitRepo};
use crate::state::sync_state::SyncState;

/// Create a sync plan by comparing local state against the remote.
pub async fn create_sync_plan(
    commitbook_dir: &Path,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    tracked_patterns: &[String],
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

    // Find dirty local files by comparing working tree vs base (from git).
    let tracked_files = list_tracked_files(repo_root, tracked_patterns)?;
    let base_files = match &repo {
        Some(r) => git::base::list(r, &base_sha)?,
        None => Vec::new(),
    };

    let mut dirty_files = Vec::new();
    for path in &tracked_files {
        let working_path = repo_root.join(path);
        let working_content = std::fs::read_to_string(&working_path)?;
        let base_content = repo
            .as_ref()
            .and_then(|r| git::base::read(r, &base_sha, path));
        match base_content {
            Some(content) if content == working_content => {} // unchanged
            _ => dirty_files.push(path.clone()), // new or modified
        }
    }

    let has_dirty = !dirty_files.is_empty();

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

    let mut planned_docs = Vec::new();

    let remote_paths: std::collections::HashSet<String> =
        remote_files.iter().cloned().collect();
    let dirty_set: std::collections::HashSet<&str> =
        dirty_files.iter().map(|s| s.as_str()).collect();

    // For each remote file, determine what action is needed.
    for remote_path in &remote_files {
        let is_dirty = dirty_set.contains(remote_path.as_str());
        let exists_locally = tracked_files.contains(remote_path)
            || base_files.contains(remote_path);

        if is_dirty {
            // Both local dirty and remote exists -> needs merge.
            planned_docs.push(PlannedDocumentSync {
                path: remote_path.clone(),
                local_state: DocumentState::Modified,
                remote_state: DocumentState::Modified,
                requires_merge: true,
                requires_conflict: false,
                requires_upload: true,
                requires_download: true,
                requires_delete: false,
            });
        } else if exists_locally {
            // Local exists but not dirty -> download if changed.
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
            });
        }
    }

    // For dirty local files not on remote.
    for dirty_path in &dirty_files {
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
            });
        }
    }

    // For base files deleted on remote (only if we had a previous checkpoint).
    if state.remote_head.is_some() {
        for base_path in &base_files {
            if !remote_paths.contains(base_path)
                && !dirty_set.contains(base_path.as_str())
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

/// List tracked files in the repo (markdown files matching patterns).
pub(crate) fn list_tracked_files(
    repo_root: &Path,
    patterns: &[String],
) -> Result<Vec<String>> {
    let mut files = Vec::new();

    if patterns.is_empty() {
        // Default: all .md files
        collect_markdown_files(repo_root, repo_root, &mut files)?;
    } else {
        // Use glob patterns from config
        for pattern in patterns {
            let full = repo_root.join(pattern).to_string_lossy().into_owned();
            for path in glob::glob(&full)
                .with_context(|| format!("Invalid glob pattern: {pattern}"))?
                .flatten()
            {
                if path.is_file() {
                    if let Ok(rel) = path.strip_prefix(repo_root) {
                        files.push(rel.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    Ok(files)
}

pub(crate) fn collect_markdown_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden directories and .CommitBook/
        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            collect_markdown_files(root, &path, files)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "markdown")
        {
            let rel = path.strip_prefix(root)?.to_string_lossy().to_string();
            files.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "planner_tests.rs"]
mod tests;
