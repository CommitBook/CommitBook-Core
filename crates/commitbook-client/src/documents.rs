//! Symlink-safe Markdown document operations inside a managed clone.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;

use commitbook_engine::git::GitRepo;
use commitbook_engine::state::RepoLock;
use sha2::{Digest, Sha256};

use crate::errors::{CommitBookError, Result};
use crate::types::{DocumentContent, DocumentSummary};

pub fn list_documents(
    workspaces_root: &Path,
    commitbook_local_id: &str,
) -> Result<Vec<DocumentSummary>> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, commitbook_local_id)?;
    let repo = GitRepo::open(&commitbook.local_path)
        .map_err(|error| CommitBookError::storage(format!("Open repo: {error}")))?;
    let changes = repo
        .changes_summary()
        .map_err(|error| CommitBookError::storage(format!("Read status: {error}")))?;
    let dirty: HashSet<&str> = changes
        .new_files
        .iter()
        .chain(changes.modified_files.iter())
        .chain(changes.deleted_files.iter())
        .map(String::as_str)
        .collect();

    let git_repo = git2::Repository::open(&commitbook.local_path)
        .map_err(|error| CommitBookError::storage(format!("Open repo: {error}")))?;
    let mut files = Vec::new();
    walk_markdown(
        &git_repo,
        &commitbook.local_path,
        &commitbook.local_path,
        &mut files,
    )?;
    files.sort();

    let conflicted: HashSet<String> = repo
        .list_conflicted_paths()
        .unwrap_or_default()
        .into_iter()
        .collect();

    files
        .into_iter()
        .map(|path| {
            let abs = crate::paths::safe_document_path(&commitbook.local_path, &path, false)?;
            let content = read_regular_nofollow(&abs)?;
            let mut hasher = Sha256::new();
            hasher.update(&content);
            Ok(DocumentSummary {
                path: path.clone(),
                dirty: dirty.contains(path.as_str()),
                has_conflicts: conflicted.contains(&path),
                checksum: hex::encode(hasher.finalize()),
            })
        })
        .collect()
}

pub fn read_document(
    workspaces_root: &Path,
    commitbook_local_id: &str,
    path: &str,
) -> Result<DocumentContent> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, commitbook_local_id)?;
    let abs = crate::paths::safe_document_path(&commitbook.local_path, path, false)?;
    if !abs.exists() {
        return Err(CommitBookError::not_found(format!(
            "Document {path} not found"
        )));
    }
    let content = String::from_utf8(read_regular_nofollow(&abs)?).map_err(|error| {
        CommitBookError::invalid_input(format!("Document {path} is not valid UTF-8: {error}"))
    })?;

    let repo = GitRepo::open(&commitbook.local_path)
        .map_err(|error| CommitBookError::storage(format!("Open repo: {error}")))?;
    let revision = repo.last_commit_touching(path).ok().flatten();
    Ok(DocumentContent {
        path: path.to_string(),
        content,
        revision,
    })
}

/// Write and commit a document. With `expected_revision` (the `revision`
/// that `read_document` returned), the save is refused when the document's
/// last commit has changed since, for example because a background sync
/// pulled another device's edit, so that edit is not silently overwritten.
pub fn save_document(
    workspaces_root: &Path,
    commitbook_local_id: &str,
    path: &str,
    content: &str,
    expected_revision: Option<&str>,
) -> Result<()> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, commitbook_local_id)?;
    let _lock = RepoLock::acquire(&commitbook.local_path)
        .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
    let repo = GitRepo::open(&commitbook.local_path)
        .map_err(|error| CommitBookError::storage(format!("Open repo: {error}")))?;
    let current_branch = repo
        .current_branch()
        .map_err(|error| CommitBookError::merge(format!("Read current branch: {error}")))?;
    if current_branch != commitbook.branch {
        return Err(CommitBookError::invalid_input(format!(
            "Cannot save {path}: checked out branch {current_branch:?} does not match configured branch {:?}",
            commitbook.branch
        )));
    }
    if repo.merge_in_progress() {
        return Err(CommitBookError::merge(
            "Cannot save a document while conflict resolution is in progress; resolve or abort the merge first",
        ));
    }
    if let Some(expected) = expected_revision {
        let current = repo
            .last_commit_touching(path)
            .map_err(|error| CommitBookError::storage(format!("Read revision: {error}")))?;
        if current.as_deref() != Some(expected) {
            return Err(CommitBookError::merge(format!(
                "{path} changed since it was read (now at {}); read it again and reapply the edit",
                current.as_deref().unwrap_or("no commit")
            )));
        }
    }
    let abs = crate::paths::safe_document_path(&commitbook.local_path, path, true)?;
    write_regular_nofollow(&abs, content.as_bytes())?;

    repo.commit_selected_paths_on_branch(
        &[path],
        &format!("Update {path} via CommitBook"),
        &commitbook.branch,
    )
    .map_err(|error| CommitBookError::storage(format!("Commit document: {error}")))?;
    Ok(())
}

fn write_regular_nofollow(path: &Path, content: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|error| {
        CommitBookError::storage(format!(
            "Open document without following symlinks {}: {error}",
            path.display()
        ))
    })?;
    file.write_all(content)
        .map_err(|error| CommitBookError::storage(format!("Write {}: {error}", path.display())))?;
    file.sync_all()
        .map_err(|error| CommitBookError::storage(format!("Sync {}: {error}", path.display())))?;
    Ok(())
}

fn read_regular_nofollow(path: &Path) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|error| {
        CommitBookError::storage(format!(
            "Open document without following symlinks {}: {error}",
            path.display()
        ))
    })?;
    let metadata = file.metadata().map_err(|error| {
        CommitBookError::storage(format!("Inspect {}: {error}", path.display()))
    })?;
    if !metadata.is_file() {
        return Err(CommitBookError::invalid_input(format!(
            "Document is not a regular file: {}",
            path.display()
        )));
    }
    let mut content = Vec::new();
    file.read_to_end(&mut content)
        .map_err(|error| CommitBookError::storage(format!("Read {}: {error}", path.display())))?;
    Ok(content)
}

fn walk_markdown(
    repository: &git2::Repository,
    root: &Path,
    directory: &Path,
    output: &mut Vec<String>,
) -> Result<()> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        CommitBookError::storage(format!("Read directory {}: {error}", directory.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            CommitBookError::storage(format!("Read entry in {}: {error}", directory.display()))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            CommitBookError::storage(format!("Inspect {}: {error}", entry.path().display()))
        })?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(|_| {
            CommitBookError::invalid_input(format!("Path escaped clone: {}", path.display()))
        })?;
        let relative_str = relative.to_str().ok_or_else(|| {
            CommitBookError::invalid_input(format!(
                "Document path is not valid UTF-8: {}",
                relative.display()
            ))
        })?;
        if relative.components().any(|component| {
            let value = component.as_os_str().to_string_lossy();
            value.eq_ignore_ascii_case(".git") || value.eq_ignore_ascii_case(".CommitBook")
        }) {
            continue;
        }
        if repository.status_should_ignore(relative).map_err(|error| {
            CommitBookError::storage(format!(
                "Check Git ignore status for {}: {error}",
                relative.display()
            ))
        })? {
            continue;
        }
        if file_type.is_dir() {
            walk_markdown(repository, root, &path, output)?;
        } else if file_type.is_file()
            && crate::paths::validate_markdown_relative(relative_str).is_ok()
        {
            output.push(relative_str.to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
