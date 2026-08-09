//! Structured conflict listing and resolution for native clients.

use std::path::Path;

use commitbook_engine::git::GitRepo;
use commitbook_engine::state::RepoLock;

use crate::errors::{CommitBookError, Result};
use crate::types::{ConflictSummary, ResolveConflictInput};

pub fn list_conflicts(workspaces_root: &Path, commitbook_id: &str) -> Result<Vec<ConflictSummary>> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, commitbook_id)?;
    let repo = GitRepo::open(&commitbook.local_path)
        .map_err(|error| CommitBookError::database(format!("Open repo: {error}")))?;
    let conflicts = repo
        .list_conflicts_structured()
        .map_err(|error| CommitBookError::database(format!("List conflicts: {error}")))?;

    let now = chrono::Utc::now().to_rfc3339();
    Ok(conflicts
        .into_iter()
        .map(|conflict| {
            let conflict_type = conflict.classification().to_string();
            let binary = conflict.is_binary_or_special();
            let ancestor_content = conflict.ancestor_text().map(ToOwned::to_owned);
            let local_content = conflict.local_text().map(ToOwned::to_owned);
            let remote_content = conflict.remote_text().map(ToOwned::to_owned);
            ConflictSummary {
                id: conflict.path.clone(),
                path: conflict.path,
                section_path: None,
                conflict_type,
                status: "open".to_string(),
                binary,
                ancestor_content,
                local_content,
                remote_content,
                opened_at: now.clone(),
            }
        })
        .collect())
}

pub fn resolve_conflict(workspaces_root: &Path, input: &ResolveConflictInput) -> Result<()> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, &input.commitbook_id)?;
    let _lock = RepoLock::acquire(&commitbook.local_path)
        .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
    let repo = GitRepo::open(&commitbook.local_path)
        .map_err(|error| CommitBookError::database(format!("Open repo: {error}")))?;
    let current_branch = repo
        .current_branch()
        .map_err(|error| CommitBookError::merge(format!("Read current branch: {error}")))?;
    if current_branch != commitbook.branch {
        return Err(CommitBookError::invalid_input(format!(
            "Cannot resolve conflicts: checked out branch {current_branch:?} does not match configured branch {:?}",
            commitbook.branch
        )));
    }
    if !repo.merge_in_progress() {
        return Err(CommitBookError::merge(
            "Cannot resolve conflicts because no merge is in progress",
        ));
    }
    let conflict = repo
        .find_conflict(&input.conflict_id)
        .map_err(|error| CommitBookError::database(format!("Read conflict: {error}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!(
                "Conflict {} not found in CommitBook {}",
                input.conflict_id, input.commitbook_id
            ))
        })?;

    match input.resolution_type.as_str() {
        "take_local" => repo
            .resolve_conflict_with_side(&input.conflict_id, conflict.local.as_ref())
            .map_err(|error| CommitBookError::merge(format!("Resolve local side: {error}")))?,
        "take_remote" => repo
            .resolve_conflict_with_side(&input.conflict_id, conflict.remote.as_ref())
            .map_err(|error| CommitBookError::merge(format!("Resolve remote side: {error}")))?,
        "delete" => repo
            .resolve_conflict_with_side(&input.conflict_id, None)
            .map_err(|error| CommitBookError::merge(format!("Resolve deletion: {error}")))?,
        "keep_both" => {
            if conflict.is_binary_or_special() {
                return Err(CommitBookError::invalid_input(format!(
                    "Conflict {} is binary or special; choose a side instead",
                    input.conflict_id
                )));
            }
            let combined = [conflict.local_text(), conflict.remote_text()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            repo.resolve_conflict_with_text(&input.conflict_id, &combined)
                .map_err(|error| CommitBookError::merge(format!("Resolve both sides: {error}")))?;
        }
        "manual_edit" => {
            let content = input.manual_content.as_deref().ok_or_else(|| {
                CommitBookError::invalid_input("manual_edit requires manual_content")
            })?;
            repo.resolve_conflict_with_text(&input.conflict_id, content)
                .map_err(|error| CommitBookError::merge(format!("Resolve manual edit: {error}")))?;
        }
        other => {
            return Err(CommitBookError::invalid_input(format!(
                "Unknown resolution_type: {other}"
            )))
        }
    }

    if repo
        .list_conflicts_structured()
        .map_err(|error| CommitBookError::database(format!("List conflicts: {error}")))?
        .is_empty()
    {
        repo.finalize_merge_commit_on_branch(None, &commitbook.branch)
            .map_err(|error| CommitBookError::merge(format!("Finalize merge: {error}")))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "conflicts_tests.rs"]
mod tests;
