//! Document operations: walking markdown files in a clone, reading their
//! content + revision SHA, and saving + staging + committing edits.
//!
//! Sync (push to remote) is `sync_commitbook`'s job — `save_document` only
//! commits locally.

use std::path::{Path, PathBuf};

use commitbook_engine::commitbooks::registry::find_by_id;
use commitbook_engine::git::GitRepo;
use sha2::{Digest, Sha256};

use crate::errors::{CommitBookError, Result};
use crate::types::{DocumentContent, DocumentSummary};

/// Walk markdown files inside a clone and return summaries.
pub fn list_documents(workspaces_root: &Path, commitbook_id: &str) -> Result<Vec<DocumentSummary>> {
    let cb = find_by_id(workspaces_root, commitbook_id)
        .map_err(|e| CommitBookError::database(format!("Registry scan: {e}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {commitbook_id} not found"))
        })?;

    let repo = GitRepo::open(&cb.local_path)
        .map_err(|e| CommitBookError::database(format!("Open repo: {e}")))?;

    // Use libgit2's status to know which markdown files are dirty.
    let status = repo
        .status_markdown()
        .map_err(|e| CommitBookError::database(format!("Read status: {e}")))?;
    let dirty_set: std::collections::HashSet<&str> = status
        .modified
        .iter()
        .chain(status.added.iter())
        .map(|s| s.as_str())
        .collect();

    // Walk the working tree for markdown files (gitignored hidden dirs skipped).
    let mut files = Vec::new();
    walk_markdown(&cb.local_path, &cb.local_path, &mut files);
    files.sort();

    let conflicted: std::collections::HashSet<String> = repo
        .list_conflicted_paths()
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut summaries = Vec::with_capacity(files.len());
    for path in files {
        let abs = cb.local_path.join(&path);
        let content = std::fs::read(&abs).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let checksum = hex::encode(hasher.finalize());
        summaries.push(DocumentSummary {
            path: path.clone(),
            dirty: dirty_set.contains(path.as_str()),
            has_conflicts: conflicted.contains(&path),
            checksum,
        });
    }
    Ok(summaries)
}

pub fn read_document(
    workspaces_root: &Path,
    commitbook_id: &str,
    path: &str,
) -> Result<DocumentContent> {
    let cb = find_by_id(workspaces_root, commitbook_id)
        .map_err(|e| CommitBookError::database(format!("Registry scan: {e}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {commitbook_id} not found"))
        })?;
    let abs = cb.local_path.join(path);
    if !abs.exists() {
        return Err(CommitBookError::not_found(format!("Document {path} not found")));
    }
    let content = std::fs::read_to_string(&abs)
        .map_err(|e| CommitBookError::database(format!("Read {path}: {e}")))?;

    // Best-effort revision lookup via libgit2: HEAD's tree contains the file's
    // last-committed blob OID. For uncommitted-only files we return None.
    let repo = GitRepo::open(&cb.local_path)
        .map_err(|e| CommitBookError::database(format!("Open repo: {e}")))?;
    let revision = repo.show_file_at_ref("HEAD", path).ok().map(|_| {
        // We need the SHA of the LAST commit touching this path; libgit2
        // doesn't expose that directly without a revwalk. For v1 we just
        // surface HEAD as a coarse "you read at this snapshot" marker.
        repo.rev_parse("HEAD").ok().unwrap_or_default()
    });

    Ok(DocumentContent {
        path: path.to_string(),
        content,
        revision,
    })
}

pub fn save_document(
    workspaces_root: &Path,
    commitbook_id: &str,
    path: &str,
    content: &str,
) -> Result<()> {
    let cb = find_by_id(workspaces_root, commitbook_id)
        .map_err(|e| CommitBookError::database(format!("Registry scan: {e}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {commitbook_id} not found"))
        })?;
    let abs = cb.local_path.join(path);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CommitBookError::database(format!("Create dirs: {e}")))?;
    }
    std::fs::write(&abs, content)
        .map_err(|e| CommitBookError::database(format!("Write {path}: {e}")))?;

    // Stage + commit (no push — sync_commitbook handles pushing).
    let repo = GitRepo::open(&cb.local_path)
        .map_err(|e| CommitBookError::database(format!("Open repo: {e}")))?;
    repo.stage_paths(&[path.to_string()])
        .map_err(|e| CommitBookError::database(format!("Stage: {e}")))?;
    if repo
        .has_real_staged_changes()
        .map_err(|e| CommitBookError::database(format!("Check changes: {e}")))?
    {
        let msg = format!("Update {path} via CommitBook");
        repo.commit(&msg)
            .map_err(|e| CommitBookError::database(format!("Commit: {e}")))?;
    }
    Ok(())
}

fn walk_markdown(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let rel_str = rel.to_string_lossy();
        // Skip hidden directories (anything starting with '.').
        if rel_str.split('/').any(|seg| seg.starts_with('.')) {
            continue;
        }
        if path.is_dir() {
            walk_markdown(root, &path, out);
        } else if let Some(ext) = path.extension() {
            let lower = ext.to_string_lossy().to_lowercase();
            if lower == "md" || lower == "markdown" {
                out.push(rel_str.to_string());
            }
        }
    }
}

// Help compiler with PathBuf references in pub fn signatures.
#[allow(dead_code)]
fn _unused_path_buf_ref(_p: PathBuf) {}
