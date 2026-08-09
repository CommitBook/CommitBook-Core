//! Document operations: walking markdown files in a clone, reading their
//! content + revision SHA, and saving + staging + committing edits.
//!
//! Sync (push to remote) is `sync_commitbook`'s job; `save_document` only
//! commits locally.

use std::path::Path;

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
    let abs = safe_rel_join(&cb.local_path, path)?;
    if !abs.exists() {
        return Err(CommitBookError::not_found(format!(
            "Document {path} not found"
        )));
    }
    let content = std::fs::read_to_string(&abs)
        .map_err(|e| CommitBookError::database(format!("Read {path}: {e}")))?;

    // Revision = SHA of the last commit that changed this specific file, so
    // callers can detect when the document moved underneath them. `None` for
    // files that exist only in the working tree (never committed).
    let repo = GitRepo::open(&cb.local_path)
        .map_err(|e| CommitBookError::database(format!("Open repo: {e}")))?;
    let revision = repo.last_commit_touching(path).ok().flatten();

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
    let abs = safe_rel_join(&cb.local_path, path)?;
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CommitBookError::database(format!("Create dirs: {e}")))?;
    }
    std::fs::write(&abs, content)
        .map_err(|e| CommitBookError::database(format!("Write {path}: {e}")))?;

    // Stage + commit (no push; sync_commitbook handles pushing).
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

/// Join a caller-supplied relative path onto the clone root, rejecting any
/// path that could escape it: absolute paths, `.`/`..` or other non-normal
/// segments, and any `.CommitBook` segment. Guards `read_document` /
/// `save_document` against path traversal from an FFI caller.
fn safe_rel_join(base: &Path, rel: &str) -> Result<std::path::PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(CommitBookError::invalid_input(format!(
            "Absolute path not allowed: {rel}"
        )));
    }
    for comp in rel_path.components() {
        match comp {
            std::path::Component::Normal(seg) => {
                // Case-insensitive: on case-insensitive filesystems (default
                // APFS/HFS+ on macOS) `.commitbook` resolves to the real
                // `.CommitBook` directory, so an exact-case check is bypassable.
                if seg.to_string_lossy().eq_ignore_ascii_case(".CommitBook") {
                    return Err(CommitBookError::invalid_input(format!(
                        "Path into .CommitBook not allowed: {rel}"
                    )));
                }
            }
            _ => {
                return Err(CommitBookError::invalid_input(format!(
                    "Illegal path segment in: {rel}"
                )));
            }
        }
    }
    Ok(base.join(rel_path))
}

fn walk_markdown(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
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

#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
