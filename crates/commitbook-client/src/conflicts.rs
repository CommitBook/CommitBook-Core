//! Structured conflict listing and resolution for native clients.

use std::path::Path;

use commitbook_engine::git::GitRepo;
use commitbook_engine::state::RepoLock;

use crate::errors::{CommitBookError, Result};
use crate::types::{ConflictSummary, ResolveConflictInput};

pub fn list_conflicts(workspaces_root: &Path, commitbook_id: &str) -> Result<Vec<ConflictSummary>> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, commitbook_id)?;
    let conflicts = commitbook_engine::review::list(&commitbook.local_path)
        .map_err(|error| CommitBookError::storage(format!("List conflicts: {error:#}")))?;

    let now = chrono::Utc::now().to_rfc3339();
    Ok(conflicts
        .into_iter()
        .map(|conflict| ConflictSummary {
            id: conflict.path.clone(),
            path: conflict.path,
            section_path: None,
            conflict_type: conflict.conflict_type,
            status: "open".to_string(),
            binary: conflict.binary,
            ancestor_content: conflict.ancestor,
            local_content: conflict.local,
            remote_content: conflict.remote,
            opened_at: now.clone(),
            revision: Some(conflict.revision),
            proposal_content: conflict.proposal.as_ref().and_then(|p| p.content.clone()),
            proposal_version: conflict.proposal_version,
            proposal_stale: conflict.proposal_stale,
            proposal_rejected: conflict.proposal.as_ref().is_some_and(|p| p.rejected),
        })
        .collect())
}

pub fn resolve_conflict(workspaces_root: &Path, input: &ResolveConflictInput) -> Result<()> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, &input.commitbook_id)?;
    let _lock = RepoLock::acquire(&commitbook.local_path)
        .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
    let repo = GitRepo::open(&commitbook.local_path)
        .map_err(|error| CommitBookError::storage(format!("Open repo: {error}")))?;
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
        .map_err(|error| CommitBookError::storage(format!("Read conflict: {error}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!(
                "Conflict {} not found in CommitBook {}",
                input.conflict_id, input.commitbook_id
            ))
        })?;

    let view = commitbook_engine::review::list(&commitbook.local_path)
        .map_err(|e| CommitBookError::merge(format!("Inspect conflicts: {e:#}")))?
        .into_iter()
        .find(|c| c.path == conflict.path)
        .ok_or_else(|| CommitBookError::not_found("Conflict disappeared"))?;
    if input.resolution_type == "accept"
        && (input.revision.is_none() || input.proposal_version.is_none())
    {
        return Err(CommitBookError::invalid_input(
            "Accepting a proposal requires its conflict revision and proposal version; refresh the conflict list",
        ));
    }
    commitbook_engine::review::apply_locked(
        &commitbook.local_path,
        &commitbook.branch,
        &commitbook_engine::review::ResolutionInput {
            proposal_version: input.proposal_version.clone(),
            path: input.conflict_id.clone(),
            revision: input.revision.clone().unwrap_or(view.revision),
            action: input.resolution_type.clone(),
            content: input.manual_content.clone(),
        },
        &_lock,
    )
    .map_err(|e| CommitBookError::merge(format!("Resolve conflict: {e:#}")))?;
    Ok(())
}

#[cfg(test)]
#[path = "conflicts_tests.rs"]
mod tests;
