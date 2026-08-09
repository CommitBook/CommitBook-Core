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

/// Initialize `.CommitBook/` in a repo directory. Persists `remote_name` into
/// the default config so the sync layer knows which remote to fetch/push.
pub fn initialize(repo_root: &Path, remote_name: &str) -> Result<()> {
    let cb_dir = repo_root.join(".CommitBook");
    prepare_local_state(repo_root)?;

    // Write default config if missing
    let config_path = cb_dir.join("config.toml");
    if !config_path.exists() {
        let mut config = crate::config::LocalConfig::new("0 * * * *");
        config.git.remote = remote_name.to_string();
        config.save(repo_root)?;
    }

    crate::config::LocalConfig::ensure_gitignore(repo_root)?;

    Ok(())
}

/// Prepare device-local state and migrate files written by pre-`local/`
/// releases. New destinations win collisions; legacy files are retained so
/// recovery is always possible.
pub fn prepare_local_state(repo_root: &Path) -> Result<()> {
    ensure_local_layout(repo_root)?;
    migrate_legacy_state(repo_root)
}

/// Create only the directories required to open the canonical lock. This is
/// deliberately separate from migration so `RepoLock` can coordinate with an
/// active legacy process before moving any state.
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

/// Move legacy state after the caller has coordinated any legacy lock.
pub(crate) fn migrate_legacy_state(repo_root: &Path) -> Result<()> {
    let cb_dir = repo_root.join(".CommitBook");
    let local = cb_dir.join("local");

    let recovery = local.join("legacy");
    for name in ["auth.toml", "state.toml"] {
        migrate_entry(&cb_dir.join(name), &local.join(name), &recovery)?;
    }

    let legacy_logs = cb_dir.join("logs");
    let local_logs = local.join("logs");
    match fs::symlink_metadata(&legacy_logs) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!(
                    "Legacy logs path is not a real directory: {}",
                    legacy_logs.display()
                );
            }
            for entry in fs::read_dir(&legacy_logs)
                .with_context(|| format!("Failed to read {}", legacy_logs.display()))?
            {
                let entry = entry?;
                let destination = local_logs.join(entry.file_name());
                let file_type = entry.file_type()?;
                if file_type.is_symlink() || !file_type.is_file() {
                    quarantine_legacy_entry(
                        &entry.path(),
                        &recovery.join("logs"),
                        &entry.file_name(),
                    )?;
                } else if let Ok(destination_metadata) = fs::symlink_metadata(&destination) {
                    if destination_metadata.file_type().is_symlink()
                        || !destination_metadata.is_file()
                    {
                        bail!(
                            "Local log destination is not a regular file: {}",
                            destination.display()
                        );
                    }
                    quarantine_legacy_entry(
                        &entry.path(),
                        &recovery.join("logs"),
                        &entry.file_name(),
                    )?;
                } else {
                    fs::rename(entry.path(), &destination).with_context(|| {
                        format!("Failed to migrate log to {}", destination.display())
                    })?;
                }
            }
            if fs::read_dir(&legacy_logs)
                .with_context(|| format!("Failed to re-read {}", legacy_logs.display()))?
                .next()
                .is_none()
            {
                fs::remove_dir(&legacy_logs).with_context(|| {
                    format!(
                        "Failed to remove empty legacy logs: {}",
                        legacy_logs.display()
                    )
                })?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Failed to inspect legacy logs: {}", legacy_logs.display())
            })
        }
    }

    Ok(())
}

fn migrate_entry(source: &Path, destination: &Path, recovery_dir: &Path) -> Result<()> {
    match fs::symlink_metadata(source) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!("Legacy state is not a regular file: {}", source.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to inspect legacy state: {}", source.display()))
        }
    }
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            let file_name = source
                .file_name()
                .context("Legacy state path has no file name")?;
            quarantine_legacy_entry(source, recovery_dir, file_name)?;
            return Ok(());
        }
        Ok(_) => bail!(
            "Local state destination is not a regular file: {}",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect local state destination: {}",
                    destination.display()
                )
            })
        }
    }
    fs::rename(source, destination).with_context(|| {
        format!(
            "Failed to migrate {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn quarantine_legacy_entry(
    source: &Path,
    recovery_dir: &Path,
    file_name: &std::ffi::OsStr,
) -> Result<PathBuf> {
    if let Some(parent) = recovery_dir.parent() {
        ensure_real_directory(parent)?;
    }
    ensure_real_directory(recovery_dir)?;
    let stem = file_name.to_string_lossy();
    let destination = (0u32..)
        .map(|suffix| {
            if suffix == 0 {
                recovery_dir.join(file_name)
            } else {
                recovery_dir.join(format!("{stem}.{suffix}"))
            }
        })
        .find(|candidate| match fs::symlink_metadata(candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Ok(_) | Err(_) => false,
        })
        .context("Could not select a unique legacy recovery path")?;
    fs::rename(source, &destination).with_context(|| {
        format!(
            "Failed to retain legacy state {} at {}",
            source.display(),
            destination.display()
        )
    })?;
    log::warn!(
        "Retained legacy state {} in ignored recovery path {}",
        source.display(),
        destination.display()
    );
    Ok(destination)
}

/// Get the local state directory (`.CommitBook/local/`) from a `.CommitBook/` dir path.
pub fn local_dir(commitbook_dir: &Path) -> PathBuf {
    commitbook_dir.join("local")
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
