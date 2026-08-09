pub mod auth;
pub mod sync_state;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

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
    let local = cb_dir.join("local");
    std::fs::create_dir_all(local.join("logs"))?;

    // Set directory permissions to 700 (owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&cb_dir, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o700));
    }

    // Write default config if missing
    let config_path = cb_dir.join("config.toml");
    if !config_path.exists() {
        let mut config = crate::config::LocalConfig::new("0 * * * *");
        config.git.remote = remote_name.to_string();
        config.save(repo_root)?;
    }

    // Update .gitignore
    update_gitignore(repo_root)?;

    Ok(())
}

/// Ensure CommitBook entries are in .gitignore.
fn update_gitignore(repo_root: &Path) -> Result<()> {
    let gitignore_path = repo_root.join(".gitignore");
    let entries = [".CommitBook/local/"];

    let content = if gitignore_path.exists() {
        std::fs::read_to_string(&gitignore_path).with_context(|| "Failed to read .gitignore")?
    } else {
        String::new()
    };

    let mut new_content = content.clone();
    let mut needs_update = false;

    for entry in &entries {
        if !new_content.lines().any(|line| line.trim() == *entry) {
            if !needs_update {
                if !new_content.is_empty() && !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
                new_content.push_str("\n# CommitBook\n");
                needs_update = true;
            }
            new_content.push_str(&format!("{}\n", entry));
        }
    }

    if needs_update {
        std::fs::write(&gitignore_path, new_content)
            .with_context(|| "Failed to update .gitignore")?;
    }

    Ok(())
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
