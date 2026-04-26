//! Blocking libgit2 operations for CommitBook creation/cloning. Run from
//! `tokio::task::spawn_blocking` — these can do filesystem I/O and network
//! operations that take seconds.

use std::path::Path;

use commitbook_engine::commitbooks::{init_dot_commitbook, slug_for};
use commitbook_engine::config::LocalConfig;
use commitbook_engine::git::GitRepo;
use commitbook_engine::platform::{CredentialProvider, TokenCredentials};

use crate::errors::{CommitBookError, Result};
use crate::types::{CommitBookInput, CommitBookSummary};

pub fn create_local_commitbook(
    workspaces_root: &Path,
    input: &CommitBookInput,
    token: &str,
) -> Result<CommitBookSummary> {
    let slug = slug_for(&input.owner, &input.repo);
    let clone_path = workspaces_root.join(&slug);

    let creds = TokenCredentials::new(token.to_string());

    if clone_path.exists() {
        // Already cloned. Re-fetch and ensure .CommitBook/ exists.
        let repo = GitRepo::open(&clone_path).map_err(|e| {
            CommitBookError::database(format!(
                "Failed to open existing clone at {}: {e}",
                clone_path.display()
            ))
        })?;
        repo.fetch_with("origin", &input.branch, &creds)
            .map_err(|e| CommitBookError::transport(format!("Fetch failed: {e}")))?;
    } else {
        let repo_url = format!("https://github.com/{}/{}.git", input.owner, input.repo);
        clone_repo_with_creds(&repo_url, &clone_path, &creds)?;
    }

    // Ensure .CommitBook/ exists committed on the remote.
    ensure_commitbook_initialized(&clone_path, input, &creds)?;

    let config = LocalConfig::load(&clone_path)
        .map_err(|e| CommitBookError::database(format!("Failed to load config: {e}")))?;

    let cb_settings = config.commitbook.unwrap_or_default();

    Ok(CommitBookSummary {
        id: format!("{}/{}", input.owner, input.repo),
        owner: input.owner.clone(),
        repo: input.repo.clone(),
        name: if cb_settings.name.is_empty() {
            input.name.clone()
        } else {
            cb_settings.name
        },
        mode: input.mode.clone(),
        provider: input.provider.clone(),
        branch: input.branch.clone(),
        auto_sync: true,
        doc_count: 0,
        conflict_count: 0,
    })
}

fn clone_repo_with_creds(
    url: &str,
    dest: &Path,
    creds: &dyn CredentialProvider,
) -> Result<()> {
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(|url, username_from_url, allowed| {
        creds
            .provide(url, username_from_url, allowed)
            .map_err(|e| git2::Error::from_str(&format!("credential provider failed: {e}")))
    });

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);

    builder
        .clone(url, dest)
        .map_err(|e| CommitBookError::transport(format!("Clone failed for {url}: {e}")))?;
    Ok(())
}

/// If the clone doesn't have `.CommitBook/config.toml` committed, write it,
/// commit, and push. Idempotent.
fn ensure_commitbook_initialized(
    clone_path: &Path,
    input: &CommitBookInput,
    creds: &dyn CredentialProvider,
) -> Result<()> {
    let dot_cb = clone_path.join(".CommitBook").join("config.toml");
    if dot_cb.exists() {
        return Ok(());
    }
    init_dot_commitbook(
        clone_path,
        &input.name,
        &input.owner,
        &input.repo,
        &input.branch,
        &input.provider,
        &input.mode,
    )
    .map_err(|e| CommitBookError::database(format!("init_dot_commitbook: {e}")))?;

    let repo = GitRepo::open(clone_path)
        .map_err(|e| CommitBookError::database(format!("Open clone: {e}")))?;
    repo.stage_all()
        .map_err(|e| CommitBookError::database(format!("Stage: {e}")))?;
    if repo
        .has_real_staged_changes()
        .map_err(|e| CommitBookError::database(format!("Diff index: {e}")))?
    {
        repo.commit("Initialize CommitBook")
            .map_err(|e| CommitBookError::database(format!("Commit: {e}")))?;
        repo.push_with("origin", &input.branch, creds)
            .map_err(|e| CommitBookError::transport(format!("Push: {e}")))?;
    }
    Ok(())
}
