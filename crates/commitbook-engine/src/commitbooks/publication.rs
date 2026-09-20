//! Metadata publication shared by CLI and SDK. Call while holding RepoLock.
use crate::state::sync_state::{PendingInitPush, SyncState};
use crate::{config::LocalConfig, git::GitRepo, platform::CredentialProvider};
use anyhow::{anyhow, Context, Result};
use std::fmt;

const METADATA_PATHS: &[&str] = &[".CommitBook/config.toml", ".CommitBook/.gitignore"];

/// Which stage of `publish_metadata` failed, so callers can label the error
/// without re-parsing messages. Commit and state failures never reached the
/// network; a push failure leaves the local commit and pending record intact.
#[derive(Debug)]
pub enum PublicationError {
    Commit(anyhow::Error),
    State(anyhow::Error),
    Push(anyhow::Error),
}

impl fmt::Display for PublicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commit(error) => write!(f, "{error:#}"),
            Self::State(error) => write!(
                f,
                "Metadata committed locally but publication state could not be saved; no push attempted, run sync to recover: {error:#}"
            ),
            Self::Push(error) => write!(
                f,
                "Metadata push failed; local commit remains intact. Fix connectivity and retry initialization, or run sync if history diverged: {error:#}"
            ),
        }
    }
}

impl std::error::Error for PublicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Commit(error) | Self::State(error) | Self::Push(error) => Some(error.as_ref()),
        }
    }
}

/// Outcome of a successful `publish_metadata` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publication {
    /// Nothing to commit and nothing pending; no network access happened.
    Unchanged,
    /// The metadata commit was pushed (now or as a retry of a pending record).
    Pushed,
    /// A pending record exists but `auto_push` is off; sync or a later init
    /// publishes it.
    Deferred,
}

/// Commit the `.CommitBook` metadata paths on `branch` and publish exactly
/// that commit to `remote`. The commit is recorded in `state.toml` before the
/// push so a failed publication can be retried without touching unrelated
/// commits. With `auto_push` false the record is kept and no push happens.
pub fn publish_metadata(
    repo: &GitRepo,
    remote: &str,
    branch: &str,
    message: &str,
    auto_push: bool,
    creds: &dyn CredentialProvider,
) -> std::result::Result<Publication, PublicationError> {
    let directory = LocalConfig::commitbook_dir(repo.path());
    let mut state = SyncState::load(&directory).map_err(PublicationError::State)?;
    if let Some(pending) = &state.pending_init_push {
        let head = repo.rev_parse("HEAD").map_err(PublicationError::Commit)?;
        if pending.remote != remote || pending.branch != branch || head != pending.commit_oid {
            return Err(PublicationError::Commit(anyhow!(
                "Pending initialization target or HEAD changed; run sync to reconcile before retrying initialization"
            )));
        }
    }
    let committed = repo
        .commit_selected_paths_on_branch(METADATA_PATHS, message, branch)
        .map_err(PublicationError::Commit)?;
    if committed.is_some() {
        state.pending_init_push = Some(PendingInitPush {
            commit_oid: repo.rev_parse("HEAD").map_err(PublicationError::Commit)?,
            remote: remote.into(),
            branch: branch.into(),
        });
        state.save(&directory).map_err(PublicationError::State)?;
    }
    let Some(pending) = &state.pending_init_push else {
        return Ok(Publication::Unchanged);
    };
    if !auto_push {
        return Ok(Publication::Deferred);
    }
    // Publish the recorded object, never a branch that external Git might have
    // advanced after validation. The remote still enforces fast-forward rules.
    repo.push_commit_with(remote, branch, &pending.commit_oid, creds)
        .map_err(PublicationError::Push)?;
    state.pending_init_push = None;
    state
        .save(&directory)
        .context("Metadata was pushed but pending state could not be cleared; retry initialization")
        .map_err(PublicationError::State)?;
    Ok(Publication::Pushed)
}

/// Clear only a pending commit proven to be contained in the published tip.
pub fn clear_published(repo: &GitRepo, remote: &str, branch: &str, tip: &str) -> Result<()> {
    let directory = LocalConfig::commitbook_dir(repo.path());
    let mut state = SyncState::load(&directory)?;
    if let Some(pending) = &state.pending_init_push {
        if pending.remote == remote
            && pending.branch == branch
            && repo.ahead_behind(&pending.commit_oid, tip)?.0 == 0
        {
            state.pending_init_push = None;
            state.save(&directory)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
