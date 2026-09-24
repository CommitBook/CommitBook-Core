use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::io::{self, IsTerminal, Write};

use commitbook_engine::commitbooks::publication::{publish_metadata, PublicationError};
use commitbook_engine::config::LocalConfig;
use commitbook_engine::devices;
use commitbook_engine::git::remote::list_remote_names;
use commitbook_engine::git::GitRepo;
use commitbook_engine::state::{self, RepoLock};

const METADATA_COMMIT_MESSAGE: &str = "Initialize CommitBook metadata for synchronized state";

fn initialize_and_publish(
    repo_root: &std::path::Path,
    remote_name: Option<&str>,
    device_name: Option<&str>,
) -> Result<()> {
    let _lock = RepoLock::acquire(repo_root)?;
    let repo = GitRepo::open(repo_root)?;
    if repo.merge_in_progress() {
        bail!(
            "Cannot publish CommitBook metadata while a merge is in progress; resolve or abort the merge, then retry `commitbook init`"
        );
    }
    let current_branch = repo.current_branch()?;
    let remote_name = match remote_name {
        Some(remote_name) => remote_name.to_string(),
        None => LocalConfig::load(repo_root)?.git.remote,
    };
    state::initialize(repo_root, &remote_name, &current_branch)?;
    let config = LocalConfig::load(repo_root)?;
    let auth = devices::desktop_auth(repo_root, &config.git.remote);
    devices::register(repo_root, device_name, auth)?;
    publish_metadata(
        &repo,
        &config.git.remote,
        &config.git.branch,
        METADATA_COMMIT_MESSAGE,
        &commitbook_engine::platform::SystemCredentials,
    )
    .map_err(|error| match error {
        PublicationError::Push(_) => anyhow::Error::new(error).context(format!(
            "CommitBook metadata was committed locally, but pushing {}/{} failed; retry `commitbook init` or `commitbook sync` after fixing authentication or remote access",
            config.git.remote, config.git.branch
        )),
        PublicationError::Commit(_) | PublicationError::State(_) => anyhow::Error::new(error),
    })?;
    Ok(())
}

/// Ask for this device's name when it is not registered yet. Returns `None`
/// (use the default) when skipped with `--yes`, without a terminal, or when
/// the answer is empty.
fn ask_device_name(repo_root: &std::path::Path, assume_yes: bool) -> Result<Option<String>> {
    if assume_yes || !io::stdin().is_terminal() || devices::this_device(repo_root)?.is_some() {
        return Ok(None);
    }
    print!(
        "Name for this device (Enter for a default like \"{}\"): ",
        devices::default_name("7f3c")
    );
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let name = input.trim();
    Ok((!name.is_empty()).then(|| name.to_string()))
}

fn confirm_publish(remote: &str, branch: &str) -> Result<bool> {
    print!("Commit and push CommitBook metadata to {remote}/{branch}? [Y/n] ");
    io::stdout().flush()?;
    let mut input = String::new();
    // EOF (Ctrl-D) declines rather than being read as a plain Enter.
    if io::stdin().read_line(&mut input)? == 0 {
        println!();
        return Ok(false);
    }
    Ok(accepts(&input))
}

/// Empty input (plain Enter) accepts; anything but `y`/`yes` declines.
fn accepts(answer: &str) -> bool {
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    )
}

/// Initialize CommitBook in the current git repo.
///
/// Requires: the current working directory is inside a git repo, and that repo
/// has exactly one remote. The remote name (whatever the user chose to call it)
/// is persisted in `.CommitBook/config.toml` so the sync layer doesn't have to
/// assume `origin`. A new initialization asks before committing and pushing
/// the metadata unless `assume_yes` is set or stdin is not a terminal.
pub fn run_init(assume_yes: bool) -> Result<()> {
    let repo_root = state::find_git_root()
        .context("CommitBook must be initialized inside a git repository.")?;

    println!("{}", commitbook_engine::inspection::INCLUSION_POLICY);
    for entry in commitbook_engine::inspection::preview_files(&repo_root)? {
        println!("  {} {}", entry.change, entry.path);
    }
    println!("Initialization publishes CommitBook metadata only; working changes are included by a later sync.");

    // Already initialized: report and exit before enforcing the remote rule,
    // so a repo that gained extra remotes after init doesn't fail here. The
    // remote persisted in config.toml stays authoritative for sync.
    if LocalConfig::exists(&repo_root) {
        // Re-run the idempotent scaffolding so a fresh clone (config.toml is
        // committed, but local/ is gitignored and therefore absent) gets its
        // local/ directory, logs, permissions, and .gitignore entry recreated.
        // Without this, scheduled sync on a clone fails for want of local/.
        // A device joining an existing CommitBook registers itself here.
        let device_name = ask_device_name(&repo_root, assume_yes)?;
        initialize_and_publish(&repo_root, None, device_name.as_deref())?;
        println!(
            "{} CommitBook is already initialized at {}",
            "OK".green().bold(),
            repo_root.join(".CommitBook").display()
        );
        print_this_device(&repo_root);
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

    // Ask before anything is written, so declining leaves the repo untouched.
    // Without a terminal (scripts, CI) proceed as if confirmed.
    if !assume_yes && io::stdin().is_terminal() {
        let branch = GitRepo::open(&repo_root)?.current_branch()?;
        if !confirm_publish(&remote_name, &branch)? {
            println!("Aborted; nothing was written.");
            return Ok(());
        }
    }

    let device_name = ask_device_name(&repo_root, assume_yes)?;
    initialize_and_publish(&repo_root, Some(&remote_name), device_name.as_deref())?;

    println!(
        "{} Initialized CommitBook in {} (remote: {})",
        "OK".green().bold(),
        repo_root.display(),
        remote_name
    );
    print_this_device(&repo_root);
    Ok(())
}

fn print_this_device(repo_root: &std::path::Path) {
    if let Ok(Some((_, device))) = devices::this_device(repo_root) {
        println!(
            "  This device: {} (rename with `commitbook devices rename <name>`)",
            device.name.cyan()
        );
    }
}

#[cfg(test)]
#[path = "init_cmd_tests.rs"]
mod tests;
