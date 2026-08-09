use anyhow::{bail, Context, Result};
use colored::Colorize;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::git::remote::list_remote_names;
use commitbook_engine::state;

/// Initialize CommitBook in the current git repo.
///
/// Requires: the current working directory is inside a git repo, and that repo
/// has exactly one remote. The remote name (whatever the user chose to call it)
/// is persisted in `.CommitBook/config.toml` so the sync layer doesn't have to
/// assume `origin`.
pub fn run_init() -> Result<()> {
    let repo_root = state::find_git_root()
        .context("CommitBook must be initialized inside a git repository.")?;

    // Already initialized: report and exit before enforcing the remote rule,
    // so a repo that gained extra remotes after init doesn't fail here. The
    // remote persisted in config.toml stays authoritative for sync.
    if LocalConfig::exists(&repo_root) {
        // Re-run the idempotent scaffolding so a fresh clone (config.toml is
        // committed, but local/ is gitignored and therefore absent) gets its
        // local/ directory, logs, permissions, and .gitignore entry recreated.
        // Without this, scheduled sync on a clone fails for want of local/.
        let config = LocalConfig::load(&repo_root)?;
        state::initialize(&repo_root, &config.git.remote)?;
        println!(
            "{} CommitBook is already initialized at {}",
            "OK".green().bold(),
            repo_root.join(".CommitBook").display()
        );
        return Ok(());
    }

    let names = list_remote_names(&repo_root)?;
    let remote_name = match names.as_slice() {
        [] => bail!(
            "CommitBook requires a git repository with exactly 1 remote. Found 0. \
             Add a remote with `git remote add <name> <url>` and try again."
        ),
        [only] => only.clone(),
        many => bail!(
            "CommitBook requires a git repository with exactly 1 remote. Found {}: {}. \
             Remove the extras with `git remote remove <name>` and try again.",
            many.len(),
            many.join(", ")
        ),
    };

    state::initialize(&repo_root, &remote_name)?;

    println!(
        "{} Initialized CommitBook in {} (remote: {})",
        "OK".green().bold(),
        repo_root.display(),
        remote_name
    );
    Ok(())
}
