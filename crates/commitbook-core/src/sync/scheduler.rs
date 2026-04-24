use anyhow::{anyhow, Result};
use std::path::Path;

use crate::domain::transport::RemoteTransport;
use crate::git::GitRepo;
use crate::logger::FileLogger;

use super::pipeline;
use super::planner;

/// Run a single sync cycle for a repository.
///
/// `commit_message` is the message used when the pipeline commits the set of
/// changes. `None` falls back to a per-file default (useful from programmatic
/// callers that don't have an AI generator available).
pub async fn sync_repository(
    commitbook_dir: &Path,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    tracked_patterns: &[String],
    logger: &FileLogger,
    commit_message: Option<String>,
) -> Result<pipeline::SyncResult> {
    migrate_legacy_local_dirs(commitbook_dir, logger)?;
    reconcile_local_branch(repo_root, branch, logger)?;

    let plan = planner::create_sync_plan(
        commitbook_dir,
        repo_root,
        branch,
        transport,
        tracked_patterns,
    )
    .await?;

    let result = pipeline::execute_sync(
        &plan,
        commitbook_dir,
        repo_root,
        branch,
        transport,
        logger,
        commit_message,
    )
    .await?;

    Ok(result)
}

/// Remove legacy CommitBook local dirs that are no longer used:
///
/// - `.CommitBook/local/remote/` — shadow clone from the pre-GitRemoteTransport era
/// - `.CommitBook/local/base/` — file-content cache replaced by git history
fn migrate_legacy_local_dirs(commitbook_dir: &Path, logger: &FileLogger) -> Result<()> {
    for legacy in ["remote", "base"] {
        let path = commitbook_dir.join("local").join(legacy);
        if path.exists() {
            let msg = format!("Removing legacy .CommitBook/local/{legacy}/");
            let _ = logger.info(&msg);
            log::info!("{}", msg);
            std::fs::remove_dir_all(&path)?;
        }
    }
    Ok(())
}

/// Fetch origin and bring the user's branch in sync with it before the planner
/// runs. Best-effort: if the repo is not a real git repo (e.g., in tests) or
/// the remote is unreachable, skip. Hard-errors only on true divergence so a
/// diverged state never silently accumulates more commits.
fn reconcile_local_branch(repo_root: &Path, branch: &str, logger: &FileLogger) -> Result<()> {
    let Ok(repo) = GitRepo::open(repo_root) else {
        return Ok(());
    };
    if !repo.has_remote() {
        return Ok(());
    }

    if let Err(e) = repo.fetch("origin", branch) {
        let _ = logger.warn(&format!("Pre-sync fetch failed: {e}"));
        log::warn!("Pre-sync fetch failed: {e}");
        return Ok(());
    }

    let remote_ref = format!("origin/{branch}");
    let (ahead, behind) = match repo.ahead_behind("HEAD", &remote_ref) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    if ahead > 0 && behind > 0 {
        return Err(anyhow!(
            "Local `{branch}` has diverged from `{remote_ref}` ({ahead} ahead, {behind} behind). \
             Run `commitbook doctor` for recovery steps."
        ));
    }

    if behind > 0 && ahead == 0 {
        if let Err(e) = repo.merge_ff_only(&remote_ref) {
            let _ = logger.warn(&format!("Pre-sync ff-merge failed: {e}"));
            log::warn!("Pre-sync ff-merge failed: {e}");
        } else {
            let _ = logger.info(&format!("Fast-forwarded local {branch} to {remote_ref}"));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "scheduler_tests.rs"]
mod tests;
