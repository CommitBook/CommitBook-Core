pub mod auth;
pub mod base;
pub mod sync_state;

use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};

/// Walk up directories to find `.CommitBook/` folder.
pub fn find_commitbook_dir() -> Result<PathBuf> {
    let mut dir =
        std::env::current_dir().context("Cannot determine current directory")?;

    loop {
        let cb_dir = dir.join(".CommitBook");
        if cb_dir.is_dir() {
            return Ok(cb_dir);
        }
        if !dir.pop() {
            bail!("Not a CommitBook repository (no .CommitBook/ found).");
        }
    }
}

/// Find `.CommitBook/` or auto-initialize if inside a git repo.
pub fn ensure_initialized() -> Result<PathBuf> {
    if let Ok(cb_dir) = find_commitbook_dir() {
        return Ok(cb_dir);
    }

    // Walk up to find .git/
    let mut dir =
        std::env::current_dir().context("Cannot determine current directory")?;

    loop {
        if dir.join(".git").exists() {
            let cb_dir = dir.join(".CommitBook");
            initialize(&dir)?;
            eprintln!(
                "{}",
                format!(
                    "Initialized CommitBook in {}",
                    dir.display()
                )
                .green()
            );
            return Ok(cb_dir);
        }
        if !dir.pop() {
            bail!("Not a git repository.");
        }
    }
}

/// Initialize `.CommitBook/` in a repo directory.
pub fn initialize(repo_root: &Path) -> Result<()> {
    let cb_dir = repo_root.join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("base"))?;
    std::fs::create_dir_all(cb_dir.join("logs"))?;

    // Set directory permissions to 700 (owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            &cb_dir,
            std::fs::Permissions::from_mode(0o700),
        );
    }

    // Write default config if missing
    let config_path = cb_dir.join("config.toml");
    if !config_path.exists() {
        let config = crate::config::LocalConfig::new("0 * * * *");
        config.save(repo_root)?;
    }

    // Update .gitignore
    update_gitignore(repo_root)?;

    Ok(())
}

/// Ensure CommitBook entries are in .gitignore.
fn update_gitignore(repo_root: &Path) -> Result<()> {
    let gitignore_path = repo_root.join(".gitignore");
    let entries = [
        ".CommitBook/auth.toml",
        ".CommitBook/state.toml",
        ".CommitBook/base/",
        ".CommitBook/logs/",
        ".CommitBook/.lock",
    ];

    let content = if gitignore_path.exists() {
        std::fs::read_to_string(&gitignore_path)
            .with_context(|| "Failed to read .gitignore")?
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
