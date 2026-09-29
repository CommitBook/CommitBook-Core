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

/// Top-level `.CommitBook/` names written only by pre-release builds, before
/// device-local state moved under `local/`: a Git token, sync state,
/// merge-base copies of notes, logs, a lock file, and the Go build's config.
/// Matched ASCII case-insensitively (the Go build wrote `Logs/`). Never list
/// a current name; when a top-level name is retired, add it here.
const LEGACY_METADATA_ENTRIES: &[&str] = &[
    "auth.toml",
    "state.toml",
    "base",
    "logs",
    ".lock",
    "config.json",
];

/// Pre-release entries at the top of `.CommitBook/`, as sorted repo-relative
/// names (`.CommitBook/logs/` for directories). Sync never adds them
/// (`git::operations::is_unpublished_metadata_path`); doctor and preview
/// report them. Read-only: never moves or removes anything.
pub fn legacy_metadata_entries(repo_root: &Path) -> Result<Vec<String>> {
    let directory = crate::config::LocalConfig::commitbook_dir(repo_root);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", directory.display()))
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("Failed to read {}", directory.display()))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if LEGACY_METADATA_ENTRIES
            .iter()
            .any(|legacy| legacy.eq_ignore_ascii_case(&name))
        {
            let slash = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                "/"
            } else {
                ""
            };
            found.push(format!(".CommitBook/{name}{slash}"));
        }
    }
    found.sort();
    Ok(found)
}

/// One-line warning for `legacy_metadata_entries`, shared by doctor and preview.
pub fn legacy_metadata_warning(entries: &[String]) -> String {
    format!(
        "Old pre-release CommitBook files: {}. Sync never adds untracked copies, but a copy already tracked in Git follows normal Git: its edits and deletions publish. They may hold a Git token or device state. Stop any old CommitBook scheduler, then delete them or move them into .CommitBook/local/.",
        entries.join(", ")
    )
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
