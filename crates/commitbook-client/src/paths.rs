//! Filesystem and identifier validation shared by every FFI operation.

use std::path::{Component, Path, PathBuf};

use commitbook_engine::commitbooks::CommitBook;

use crate::errors::{CommitBookError, Result};

pub fn canonicalize_workspaces_root(root: &Path) -> Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(root).map_err(|error| {
        CommitBookError::storage(format!(
            "Inspect workspaces root {}: {error}",
            root.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommitBookError::invalid_input(format!(
            "Workspaces root must be a real directory, not a symlink: {}",
            root.display()
        )));
    }
    root.canonicalize().map_err(|error| {
        CommitBookError::storage(format!(
            "Canonicalize workspaces root {}: {error}",
            root.display()
        ))
    })
}

pub fn validate_repo_component(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.bytes().all(|byte| byte == b'.')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(CommitBookError::invalid_input(format!(
            "Invalid GitHub {label}: {value:?}"
        )));
    }
    Ok(())
}

pub fn validate_branch(branch: &str) -> Result<()> {
    let refname = format!("refs/heads/{branch}");
    if branch.is_empty()
        || branch
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
        || !git2::Reference::is_valid_name(&refname)
    {
        return Err(CommitBookError::invalid_input(format!(
            "Invalid branch name: {branch:?}"
        )));
    }
    Ok(())
}

pub fn validate_provider(provider: &str) -> Result<()> {
    if provider != "github" {
        return Err(CommitBookError::invalid_input("Native cloning supports only provider github; use local registration for existing clones"));
    }
    Ok(())
}

/// Discovery matches remote metadata, even if a clone's local identity needs repair.
pub fn github_remote_is_local(root: &Path, owner: &str, repo: &str) -> bool {
    let Ok(root) = canonicalize_workspaces_root(root) else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let Ok(path) = validate_managed_clone(&root, &entry.path()) else {
            return false;
        };
        let Ok(config) = commitbook_engine::config::LocalConfig::load(&path) else {
            return false;
        };
        commitbook_engine::git::remote::remote_identity(&path, &config.git.remote).is_ok_and(
            |identity| {
                matches!(
                    identity.host.as_deref(),
                    Some("github.com" | "www.github.com")
                ) && identity.owner.eq_ignore_ascii_case(owner)
                    && identity.repo.eq_ignore_ascii_case(repo)
            },
        )
    })
}

pub fn validate_init_input(owner: &str, repo: &str, branch: &str) -> Result<()> {
    validate_repo_component("owner", owner)?;
    validate_repo_component("repository", repo)?;
    validate_branch(branch)
}

pub fn find_managed_commitbook(root: &Path, id: &str) -> Result<CommitBook> {
    let commitbook = commitbook_engine::commitbooks::registry::find_by_id(root, id)
        .map_err(|error| CommitBookError::storage(format!("Registry scan: {error}")))?
        .ok_or_else(|| CommitBookError::not_found(format!("CommitBook {id} not found")))?;
    validate_managed_clone(root, &commitbook.local_path)?;
    Ok(commitbook)
}

/// Require a clone to be a real direct child directory of the canonical root.
pub fn validate_managed_clone(root: &Path, clone_path: &Path) -> Result<PathBuf> {
    let root = canonicalize_workspaces_root(root)?;
    if clone_path.parent() != Some(root.as_path()) || clone_path.file_name().is_none() {
        return Err(CommitBookError::invalid_input(format!(
            "Managed clone must be a direct child of {}: {}",
            root.display(),
            clone_path.display()
        )));
    }
    let metadata = std::fs::symlink_metadata(clone_path).map_err(|error| {
        CommitBookError::storage(format!("Inspect clone {}: {error}", clone_path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommitBookError::invalid_input(format!(
            "Managed clone is not a real directory: {}",
            clone_path.display()
        )));
    }
    let canonical = clone_path.canonicalize().map_err(|error| {
        CommitBookError::storage(format!(
            "Canonicalize clone {}: {error}",
            clone_path.display()
        ))
    })?;
    if canonical.parent() != Some(root.as_path()) {
        return Err(CommitBookError::invalid_input(format!(
            "Managed clone escapes workspaces root: {}",
            clone_path.display()
        )));
    }
    Ok(canonical)
}

pub fn validate_clone_destination(root: &Path, destination: &Path) -> Result<()> {
    let root = canonicalize_workspaces_root(root)?;
    if destination.parent() != Some(root.as_path()) || destination.file_name().is_none() {
        return Err(CommitBookError::invalid_input(format!(
            "Clone destination must be a direct child of {}",
            root.display()
        )));
    }
    match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CommitBookError::invalid_input(
            format!("Clone destination is a symlink: {}", destination.display()),
        )),
        Ok(metadata) if !metadata.is_dir() => Err(CommitBookError::invalid_input(format!(
            "Clone destination is not a directory: {}",
            destination.display()
        ))),
        Ok(_) => {
            validate_managed_clone(&root, destination)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CommitBookError::storage(format!(
            "Inspect clone destination {}: {error}",
            destination.display()
        ))),
    }
}

pub fn validate_markdown_relative(path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(CommitBookError::invalid_input(format!(
            "Document path must be a non-empty relative path: {path:?}"
        )));
    }
    // `Path::components` normalizes away repeated separators and `.` segments.
    // Rebuild the accepted path from its normal components and compare it with
    // the caller's exact spelling so aliases such as `notes/./day.md` cannot
    // pass validation under a different lexical path.
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(CommitBookError::invalid_input(format!(
                "Illegal document path segment: {path}"
            )));
        };
        normalized.push(segment);
        let segment = segment.to_string_lossy();
        if segment.eq_ignore_ascii_case(".git") || segment.eq_ignore_ascii_case(".CommitBook") {
            return Err(CommitBookError::invalid_input(format!(
                "Document path enters a protected Git directory: {path}"
            )));
        }
    }
    if normalized.as_os_str() != relative.as_os_str() {
        return Err(CommitBookError::invalid_input(format!(
            "Document path is not in normal form: {path}"
        )));
    }
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("md") && !extension.eq_ignore_ascii_case("markdown") {
        return Err(CommitBookError::invalid_input(format!(
            "Document path must end in .md or .markdown: {path}"
        )));
    }
    Ok(normalized)
}

/// Resolve a Markdown path without following any symlink component. Missing
/// parents are created one at a time only for writes.
pub fn safe_document_path(base: &Path, path: &str, create_parents: bool) -> Result<PathBuf> {
    let relative = validate_markdown_relative(path)?;
    let base = base.canonicalize().map_err(|error| {
        CommitBookError::storage(format!("Canonicalize clone {}: {error}", base.display()))
    })?;
    let mut current = base.clone();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(segment) = component else {
            unreachable!("validate_markdown_relative accepted only normal components")
        };
        current.push(segment);
        let is_final = index + 1 == components.len();
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(CommitBookError::invalid_input(format!(
                        "Document path traverses a symlink: {path}"
                    )));
                }
                if is_final && !metadata.is_file() {
                    return Err(CommitBookError::invalid_input(format!(
                        "Document is not a regular file: {path}"
                    )));
                }
                if !is_final && !metadata.is_dir() {
                    return Err(CommitBookError::invalid_input(format!(
                        "Document parent is not a directory: {path}"
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !is_final {
                    if !create_parents {
                        return Err(CommitBookError::not_found(format!(
                            "Document {path} not found"
                        )));
                    }
                    std::fs::create_dir(&current).map_err(|error| {
                        CommitBookError::storage(format!(
                            "Create document directory {}: {error}",
                            current.display()
                        ))
                    })?;
                }
            }
            Err(error) => {
                return Err(CommitBookError::storage(format!(
                    "Inspect document path {}: {error}",
                    current.display()
                )))
            }
        }
        if current.exists() {
            let canonical = current.canonicalize().map_err(|error| {
                CommitBookError::storage(format!(
                    "Canonicalize document path {}: {error}",
                    current.display()
                ))
            })?;
            if !canonical.starts_with(&base) {
                return Err(CommitBookError::invalid_input(format!(
                    "Document path escapes clone: {path}"
                )));
            }
        }
    }
    Ok(current)
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
