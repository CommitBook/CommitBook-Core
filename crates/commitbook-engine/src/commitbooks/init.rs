use anyhow::{Context, Result};
use std::path::Path;

use crate::config::{CommitBookSettings, LocalConfig};

/// Initialize `.CommitBook/` inside a freshly-cloned (or existing) repo
/// with metadata identifying it as a CommitBook.
///
/// Writes `.CommitBook/config.toml` with the `[commitbook]` section
/// populated, ensures `.CommitBook/local/` exists, and adds it to the
/// repo's `.gitignore`. Does NOT commit — caller decides commit timing
/// (typically: stage + commit + push immediately after, so the
/// `.CommitBook/` marker shows up on the remote).
///
/// Idempotent: if `.CommitBook/config.toml` already exists, the function
/// preserves all existing settings and only fills in the `[commitbook]`
/// section if it's missing.
pub fn init_dot_commitbook(
    repo_root: &Path,
    name: &str,
    owner: &str,
    repo: &str,
    branch: &str,
    provider: &str,
    mode: &str,
) -> Result<()> {
    let cb_dir = repo_root.join(".CommitBook");
    let local = cb_dir.join("local");
    std::fs::create_dir_all(local.join("logs"))
        .with_context(|| format!("Failed to create {}", local.join("logs").display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&cb_dir, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o700));
    }

    let config_path = cb_dir.join("config.toml");
    let mut config = if config_path.exists() {
        LocalConfig::load(repo_root)?
    } else {
        let mut c = LocalConfig::new("0 * * * *");
        c.git.branch = branch.to_string();
        c
    };

    if config.commitbook.is_none() {
        config.commitbook = Some(CommitBookSettings {
            name: name.to_string(),
            owner: owner.to_string(),
            repo: repo.to_string(),
            provider: provider.to_string(),
            mode: mode.to_string(),
        });
    }
    config.git.branch = branch.to_string();
    config.save(repo_root)?;

    update_gitignore(repo_root)?;

    Ok(())
}

fn update_gitignore(repo_root: &Path) -> Result<()> {
    let gitignore_path = repo_root.join(".gitignore");
    let entry = ".CommitBook/local/";
    let content = if gitignore_path.exists() {
        std::fs::read_to_string(&gitignore_path).context("Failed to read .gitignore")?
    } else {
        String::new()
    };

    if content.lines().any(|l| l.trim() == entry) {
        return Ok(());
    }

    let mut new_content = content;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    if !new_content.contains("# CommitBook") {
        new_content.push_str("\n# CommitBook\n");
    }
    new_content.push_str(entry);
    new_content.push('\n');
    std::fs::write(&gitignore_path, new_content).context("Failed to write .gitignore")?;
    Ok(())
}
