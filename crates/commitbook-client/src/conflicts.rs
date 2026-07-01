//! Conflict surface: list unmerged paths after a `Manual`-mode sync left
//! markers behind, and resolve them by writing the chosen content + staging.
//!
//! Conflict IDs are stable per-clone derived from the path: a sync that
//! leaves `notes/2026-04-25.md` conflicted gets id `notes/2026-04-25.md`.
//! Idempotent: re-listing returns the same IDs as long as the path is
//! still in the index's conflict set.

use std::path::Path;

use commitbook_engine::commitbooks::registry::find_by_id;
use commitbook_engine::git::GitRepo;

use crate::errors::{CommitBookError, Result};
use crate::types::{ConflictSummary, ResolveConflictInput};

const SECTION_LOCAL_MARKER: &str = "<<<<<<<";
const SECTION_BASE_MARKER: &str = "|||||||";
const SECTION_DIVIDER: &str = "=======";
const SECTION_REMOTE_MARKER: &str = ">>>>>>>";

pub fn list_conflicts(
    workspaces_root: &Path,
    commitbook_id: &str,
) -> Result<Vec<ConflictSummary>> {
    let cb = find_by_id(workspaces_root, commitbook_id)
        .map_err(|e| CommitBookError::database(format!("Registry scan: {e}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {commitbook_id} not found"))
        })?;
    let repo = GitRepo::open(&cb.local_path)
        .map_err(|e| CommitBookError::database(format!("Open repo: {e}")))?;
    let paths = repo
        .list_conflicted_paths()
        .map_err(|e| CommitBookError::database(format!("List conflicts: {e}")))?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let abs = cb.local_path.join(&path);
        let raw = std::fs::read_to_string(&abs).unwrap_or_default();
        let (local_content, remote_content) = split_conflict_markers(&raw);
        out.push(ConflictSummary {
            id: path.clone(),
            path,
            section_path: None,
            conflict_type: "file_conflict".to_string(),
            status: "open".to_string(),
            local_content,
            remote_content,
            opened_at: now.clone(),
        });
    }
    Ok(out)
}

pub fn resolve_conflict(workspaces_root: &Path, input: &ResolveConflictInput) -> Result<()> {
    // The caller identifies the exact CommitBook, so there's no ambiguity when
    // two clones happen to be conflicted on the same relative path.
    let cb = find_by_id(workspaces_root, &input.commitbook_id)
        .map_err(|e| CommitBookError::database(format!("Registry scan: {e}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {} not found", input.commitbook_id))
        })?;

    let repo = GitRepo::open(&cb.local_path)
        .map_err(|e| CommitBookError::database(format!("Open repo: {e}")))?;
    let conflicts = repo
        .list_conflicted_paths()
        .map_err(|e| CommitBookError::database(format!("List conflicts: {e}")))?;
    if !conflicts.contains(&input.conflict_id) {
        return Err(CommitBookError::not_found(format!(
            "Conflict {} not found in CommitBook {}",
            input.conflict_id, input.commitbook_id
        )));
    }

    let abs = cb.local_path.join(&input.conflict_id);
    let raw = std::fs::read_to_string(&abs)
        .map_err(|e| CommitBookError::database(format!("Read {}: {e}", input.conflict_id)))?;
    let (local_content, remote_content) = split_conflict_markers(&raw);

    let resolved = match input.resolution_type.as_str() {
        "take_local" => local_content,
        "take_remote" => remote_content,
        "keep_both" => format!("{}\n{}", local_content, remote_content),
        "manual_edit" => input
            .manual_content
            .clone()
            .ok_or_else(|| CommitBookError::invalid_input(
                "manual_edit requires manual_content",
            ))?,
        other => {
            return Err(CommitBookError::invalid_input(format!(
                "Unknown resolution_type: {other}"
            )))
        }
    };

    std::fs::write(&abs, resolved)
        .map_err(|e| CommitBookError::database(format!("Write resolved: {e}")))?;
    repo.stage_paths(&[input.conflict_id.clone()])
        .map_err(|e| CommitBookError::database(format!("Stage: {e}")))?;

    // If all conflicts cleared, finalize the merge commit.
    let remaining = repo
        .list_conflicted_paths()
        .map_err(|e| CommitBookError::database(format!("List conflicts: {e}")))?;
    if remaining.is_empty() {
        repo.finalize_merge_commit(None)
            .map_err(|e| CommitBookError::database(format!("Finalize merge: {e}")))?;
    }
    Ok(())
}

/// Extract the local and remote sides of git's conflict markers from a file.
/// If the file has no markers, both sides equal the raw content.
fn split_conflict_markers(raw: &str) -> (String, String) {
    let mut local = String::new();
    let mut remote = String::new();
    // 0=common, 1=local, 2=remote, 3=base (diff3 ancestor, dropped).
    let mut state = 0u8;
    let mut has_markers = false;
    for line in raw.lines() {
        if line.starts_with(SECTION_LOCAL_MARKER) {
            has_markers = true;
            state = 1;
            continue;
        }
        // `merge.conflictStyle = diff3`/`zdiff3` inserts a `||||||| base`
        // section between the local side and the divider; skip it entirely.
        if line.starts_with(SECTION_BASE_MARKER) && state == 1 {
            state = 3;
            continue;
        }
        if line.starts_with(SECTION_DIVIDER) && (state == 1 || state == 3) {
            state = 2;
            continue;
        }
        if line.starts_with(SECTION_REMOTE_MARKER) && state == 2 {
            state = 0;
            continue;
        }
        match state {
            0 => {
                local.push_str(line);
                local.push('\n');
                remote.push_str(line);
                remote.push('\n');
            }
            1 => {
                local.push_str(line);
                local.push('\n');
            }
            2 => {
                remote.push_str(line);
                remote.push('\n');
            }
            // 3 = base/ancestor: belongs to neither side.
            _ => {}
        }
    }
    if !has_markers {
        return (raw.to_string(), raw.to_string());
    }
    (local, remote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_no_markers() {
        let (l, r) = split_conflict_markers("hello\nworld\n");
        assert_eq!(l, "hello\nworld\n");
        assert_eq!(r, "hello\nworld\n");
    }

    #[test]
    fn split_simple_conflict() {
        let raw = "intro\n<<<<<<< HEAD\nlocal line\n=======\nremote line\n>>>>>>> origin/main\noutro\n";
        let (l, r) = split_conflict_markers(raw);
        assert_eq!(l, "intro\nlocal line\noutro\n");
        assert_eq!(r, "intro\nremote line\noutro\n");
    }

    #[test]
    fn split_diff3_conflict_drops_ancestor() {
        // diff3/zdiff3 style: a `||||||| base` section sits between the local
        // side and the divider. It must not leak into either resolved side.
        let raw = "intro\n<<<<<<< HEAD\nlocal line\n||||||| base\nancestor line\n=======\nremote line\n>>>>>>> origin/main\noutro\n";
        let (l, r) = split_conflict_markers(raw);
        assert_eq!(l, "intro\nlocal line\noutro\n");
        assert_eq!(r, "intro\nremote line\noutro\n");
    }
}
