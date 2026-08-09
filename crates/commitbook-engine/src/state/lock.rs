use anyhow::{Context, Result};
use fs2::FileExt;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::config::LocalConfig;

/// Returned when another process already owns a repository mutation lock.
#[derive(Debug)]
pub struct RepoLockContended {
    path: PathBuf,
}

impl fmt::Display for RepoLockContended {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "another CommitBook operation is already running for {}",
            self.path.display()
        )
    }
}

impl std::error::Error for RepoLockContended {}

/// Process-wide repository mutation guard backed by an advisory file lock.
///
/// The lock file is intentionally retained after release. Removing a lock
/// path while a process still holds its inode can let another process acquire
/// a newly-created file and enter the critical section concurrently.
#[derive(Debug)]
pub struct RepoLock {
    repo_root: PathBuf,
    primary: File,
    legacy: Option<File>,
}

impl RepoLock {
    pub fn acquire(repo_root: &Path) -> Result<Self> {
        let repo_root = repo_root.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize repository root: {}",
                repo_root.display()
            )
        })?;
        crate::state::ensure_local_layout(&repo_root)?;

        // Coordinate with releases that locked `.CommitBook/.lock` before
        // taking the canonical `.CommitBook/local/.lock`.
        let legacy_path = LocalConfig::commitbook_dir(&repo_root).join(".lock");
        let primary_path = LocalConfig::lock_path(&repo_root);
        let legacy = if inspect_lock_path(&legacy_path)? {
            Some(open_and_lock(&legacy_path, &repo_root)?)
        } else {
            None
        };
        let _ = inspect_lock_path(&primary_path)?;
        let primary = open_and_lock(&primary_path, &repo_root)?;
        let lock = Self {
            repo_root: repo_root.clone(),
            primary,
            legacy,
        };
        crate::state::migrate_legacy_state(&repo_root)?;
        if lock.legacy.is_some() {
            std::fs::remove_file(&legacy_path).with_context(|| {
                format!("Failed to remove legacy lock: {}", legacy_path.display())
            })?;
        }
        Ok(lock)
    }

    pub(crate) fn ensure_matches(&self, repo_root: &Path) -> Result<()> {
        let requested = repo_root.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize repository root: {}",
                repo_root.display()
            )
        })?;
        if requested != self.repo_root {
            anyhow::bail!(
                "Repository lock for {} cannot guard operation on {}",
                self.repo_root.display(),
                requested.display()
            );
        }
        Ok(())
    }
}

fn inspect_lock_path(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => anyhow::bail!("Repository lock is not a regular file: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to inspect repository lock: {}", path.display())),
    }
}

fn open_and_lock(path: &Path, repo_root: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .with_context(|| format!("Failed to open repository lock: {}", path.display()))?;
    let path_metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect repository lock: {}", path.display()))?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        anyhow::bail!("Repository lock is not a regular file: {}", path.display());
    }
    if !file.metadata()?.is_file() {
        anyhow::bail!(
            "Opened repository lock is not a regular file: {}",
            path.display()
        );
    }
    match file.try_lock_exclusive() {
        Ok(()) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Err(RepoLockContended {
            path: repo_root.to_path_buf(),
        }
        .into()),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to acquire repository lock: {}", path.display())),
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        if let Some(legacy) = &self.legacy {
            let _ = legacy.unlock();
        }
        let _ = self.primary.unlock();
    }
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
