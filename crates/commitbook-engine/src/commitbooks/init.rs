use anyhow::Result;
use std::path::Path;

use crate::config::{CommitBookSettings, LocalConfig};

/// Initialize `.CommitBook/` inside a freshly-cloned (or existing) repo
/// with metadata identifying it as a CommitBook.
///
/// Writes `.CommitBook/config.toml` with the `[commitbook]` section
/// populated, ensures `.CommitBook/local/` exists, and protects it with
/// `.CommitBook/.gitignore`. Does NOT commit, caller decides commit timing
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
    crate::state::prepare_local_state(repo_root)?;
    let existing_config = LocalConfig::exists(repo_root);
    let mut config = if existing_config {
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
    // Existing repositories keep their configured sync branch. The caller's
    // branch only seeds a brand-new config.
    config.save(repo_root)?;

    LocalConfig::ensure_gitignore(repo_root)?;

    Ok(())
}
