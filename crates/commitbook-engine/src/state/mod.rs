pub mod auth;
pub mod lock;
pub mod sync_state;

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub use lock::{RepoLock, RepoLockContended};

/// Sentinel message used when `.CommitBook/` is missing. CLI layer matches on
/// this to present a friendly "run `commitbook init` first" error.
pub const NOT_INITIALIZED_MESSAGE: &str =
    "CommitBook is not initialized. Run `commitbook init` first.";

/// Walk up directories to find `.CommitBook/` folder.
pub fn find_commitbook_dir() -> Result<PathBuf> {
    let start = std::env::current_dir().context("Cannot determine current directory")?;
    find_commitbook_dir_from(&start)
}

fn find_commitbook_dir_from(start: &Path) -> Result<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let cb_dir = dir.join(".CommitBook");
        if cb_dir.is_dir() && dir.join(".git").exists() {
            return Ok(cb_dir);
        }
        if !dir.pop() {
            bail!("{}", NOT_INITIALIZED_MESSAGE);
        }
    }
}

/// Walk up from `start` looking for a `.git/` directory and return its parent.
pub fn find_git_root_from(start: &Path) -> Result<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("Not a git repository.");
        }
    }
}

/// Walk up from the current directory looking for a `.git/` directory.
pub fn find_git_root() -> Result<PathBuf> {
    let start = std::env::current_dir().context("Cannot determine current directory")?;
    find_git_root_from(&start)
}

/// Find the `.CommitBook/` directory for the current repo, or return
/// `StateError::NotInitialized` if none is set up. Does NOT auto-initialize.
pub fn ensure_initialized() -> Result<PathBuf> {
    find_commitbook_dir()
}

/// Initialize `.CommitBook/` in a repo directory. A missing config is
/// written with defaults for `remote_name` and `branch`, named after the
/// repository in the remote URL (or the folder when the URL has none).
pub fn initialize(repo_root: &Path, remote_name: &str, branch: &str) -> Result<()> {
    prepare_local_state(repo_root)?;

    if !crate::config::LocalConfig::exists(repo_root) {
        let name = crate::git::remote::remote_identity(repo_root, remote_name)
            .map(|identity| identity.repo)
            .ok()
            .or_else(|| {
                repo_root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "CommitBook".to_string());
        crate::config::LocalConfig::new(&name, branch, remote_name).save(repo_root)?;
    }

    crate::config::LocalConfig::ensure_gitignore(repo_root)?;

    Ok(())
}

/// Reject unknown metadata before sync can stage or publish abandoned state.
/// This validates the current layout only; it never moves or removes anything.
pub fn validate_metadata_layout(repo_root: &Path) -> Result<()> {
    let directory = crate::config::LocalConfig::commitbook_dir(repo_root);
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        if !matches!(
            entry.file_name().to_str(),
            Some("config.toml" | ".gitignore" | "devices" | "local")
        ) {
            bail!("Unexpected CommitBook metadata entry {}; refusing sync to avoid publishing local state. Back up the development layout and initialize a fresh clone", entry.path().display());
        }
    }
    Ok(())
}

/// Prepare only the current device-local layout. Never migrates old files.
pub fn prepare_local_state(repo_root: &Path) -> Result<()> {
    ensure_local_layout(repo_root)
}

/// Create directories required for the current repository lock and state.
pub(crate) fn ensure_local_layout(repo_root: &Path) -> Result<()> {
    let cb_dir = repo_root.join(".CommitBook");
    let local = cb_dir.join("local");
    ensure_real_directory(&cb_dir)?;
    ensure_real_directory(&local)?;
    ensure_real_directory(&local.join("logs"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&cb_dir, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(&local, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => bail!(
            "Local state path is not a real directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                match fs::symlink_metadata(path) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        Ok(())
                    }
                    Ok(_) => bail!(
                        "Local state path is not a real directory: {}",
                        path.display()
                    ),
                    Err(error) => Err(error).with_context(|| {
                        format!("Failed to revalidate local state: {}", path.display())
                    }),
                }
            }
            Err(error) => Err(error)
                .with_context(|| format!("Failed to create local state: {}", path.display())),
        },
        Err(error) => {
            Err(error).with_context(|| format!("Failed to inspect local state: {}", path.display()))
        }
    }
}

/// Get the repo root from a `.CommitBook/` dir path.
pub fn repo_root(commitbook_dir: &Path) -> PathBuf {
    commitbook_dir
        .parent()
        .expect(".CommitBook must have a parent")
        .to_path_buf()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
