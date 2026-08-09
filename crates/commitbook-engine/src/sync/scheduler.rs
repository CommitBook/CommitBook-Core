//! Merge-based sync orchestrator over libgit2.
//!
//! Executes one cycle of: commit dirty non-ignored changes → fetch + merge →
//! AI conflict resolution (if configured) or surface conflicts to caller →
//! optionally push (retry once on race). Single code path on desktop and mobile,
//! see `docs/details/sync-architecture.md`.

use anyhow::Result;
use std::path::Path;

use crate::ai::{ConflictResolution, ConflictResolver, ResolverRegistry};
use crate::config::{local::GitSettings, LocalConfig};
use crate::git::operations::MergeOutcome;
use crate::git::GitRepo;
use crate::platform::{CredentialProvider, Logger, SystemCredentials};
use crate::state::sync_state::SyncState;
use crate::state::RepoLock;

const MAX_PUSH_RETRIES: u32 = 1;

/// Git policy for one sync cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOptions {
    pub remote: String,
    pub branch: String,
    pub auto_push: bool,
}

impl SyncOptions {
    pub fn new(remote: impl Into<String>, branch: impl Into<String>, auto_push: bool) -> Self {
        Self {
            remote: remote.into(),
            branch: branch.into(),
            auto_push,
        }
    }
}

impl From<&GitSettings> for SyncOptions {
    fn from(settings: &GitSettings) -> Self {
        Self {
            remote: settings.remote.clone(),
            branch: settings.branch.clone(),
            auto_push: settings.auto_push,
        }
    }
}

/// Result of one sync cycle.
#[derive(Debug, Default)]
pub struct SyncOutcome {
    pub committed: bool,
    pub pushed: u32,
    pub pulled: u32,
    pub conflicts_resolved: u32,
    pub manual_conflicts: u32,
    pub errors: Vec<String>,
}

impl SyncOutcome {
    pub fn is_clean(&self) -> bool {
        !self.committed
            && self.pushed == 0
            && self.pulled == 0
            && self.conflicts_resolved == 0
            && self.manual_conflicts == 0
            && self.errors.is_empty()
    }
}

/// Run one sync cycle.
///
/// Loads the configured conflict resolver from the registry; programmatic
/// callers wanting to inject a fake resolver use `sync_with_resolver` directly.
pub async fn sync_repository(
    repo_root: &Path,
    config: &LocalConfig,
    logger: &dyn Logger,
    commit_message: Option<String>,
) -> Result<SyncOutcome> {
    let lock = RepoLock::acquire(repo_root)?;
    sync_repository_locked(repo_root, config, logger, commit_message, &lock).await
}

/// Run a sync cycle while using a repository lock already held by the caller.
/// This lets frontends acquire the lock before loading config, opening logs, or
/// invoking an AI commit-message provider.
pub async fn sync_repository_locked(
    repo_root: &Path,
    config: &LocalConfig,
    logger: &dyn Logger,
    commit_message: Option<String>,
    lock: &RepoLock,
) -> Result<SyncOutcome> {
    lock.ensure_matches(repo_root)?;
    let registry = ResolverRegistry::new();
    let resolver = registry.get(&config.conflict.resolver);
    let options = SyncOptions::from(&config.git);
    sync_with_resolver_locked(
        repo_root,
        &options,
        resolver,
        &SystemCredentials,
        logger,
        commit_message,
        lock,
    )
    .await
}

/// Lower-level entry that accepts an explicit resolver (or `None` for
/// manual mode). Used by `sync_repository` and by tests.
pub async fn sync_with_resolver(
    repo_root: &Path,
    options: &SyncOptions,
    resolver: Option<&dyn ConflictResolver>,
    creds: &dyn CredentialProvider,
    logger: &dyn Logger,
    commit_message: Option<String>,
) -> Result<SyncOutcome> {
    let lock = RepoLock::acquire(repo_root)?;
    sync_with_resolver_locked(
        repo_root,
        options,
        resolver,
        creds,
        logger,
        commit_message,
        &lock,
    )
    .await
}

/// Lower-level sync entry for a caller that already owns the matching lock.
pub async fn sync_with_resolver_locked(
    repo_root: &Path,
    options: &SyncOptions,
    resolver: Option<&dyn ConflictResolver>,
    creds: &dyn CredentialProvider,
    logger: &dyn Logger,
    commit_message: Option<String>,
    lock: &RepoLock,
) -> Result<SyncOutcome> {
    lock.ensure_matches(repo_root)?;
    let repo = GitRepo::open(repo_root)?;
    let current_branch = repo.current_branch()?;
    if current_branch != options.branch {
        anyhow::bail!(
            "Cannot sync while checked out on branch {:?}: CommitBook is configured for {:?}. Check out the configured branch and retry.",
            current_branch,
            options.branch
        );
    }
    let cb_dir = LocalConfig::commitbook_dir(repo_root);
    let mut outcome = SyncOutcome::default();

    for attempt in 0..=MAX_PUSH_RETRIES {
        // 0. Recover from a merge left in progress by a previous manual-mode
        //    or failed-resolver cycle before touching the working tree.
        //    Guarding on merge_in_progress() makes this a no-op on retries.
        if repo.merge_in_progress() {
            let unresolved = repo.list_conflicted_paths()?;
            if !unresolved.is_empty() {
                match resolver {
                    Some(resolver) => {
                        if let Err(error) =
                            resolve_conflicts_inner(&repo, repo_root, &unresolved, resolver, logger)
                                .await
                        {
                            record_resolver_failure(
                                &repo,
                                &unresolved,
                                &mut outcome,
                                &error,
                                logger,
                            )?;
                            return Ok(finalize_outcome(&cb_dir, outcome));
                        }
                        repo.finalize_merge_commit_on_branch(None, &options.branch)?;
                        outcome.conflicts_resolved += unresolved.len() as u32;
                    }
                    None => {
                        outcome.manual_conflicts += unresolved.len() as u32;
                        let msg = format!(
                            "{} conflict(s) still need manual resolution: {}",
                            unresolved.len(),
                            unresolved.join(", ")
                        );
                        let _ = logger.warn(&msg);
                        outcome.errors.push(msg);
                        return Ok(finalize_outcome(&cb_dir, outcome));
                    }
                }
            } else {
                // Markers already resolved by the user: complete the merge.
                repo.finalize_merge_commit_on_branch(None, &options.branch)?;
            }
        }

        // 1. Commit every dirty, non-ignored Git change FIRST. Doing this
        //    before the merge means
        //    a) we never lose work to a failed merge, b) the merge sees a
        //    proper local commit if the dirty file overlaps with remote.
        if repo.has_dirty_changes()? {
            repo.stage_all()?;
            if repo.has_real_staged_changes()? {
                let msg = commit_message.clone().unwrap_or_else(|| {
                    format!(
                        "Update via CommitBook ({})",
                        crate::utils::datetime::now_iso()
                    )
                });
                repo.commit_on_branch(&msg, &options.branch)?;
                outcome.committed = true;
            }
        }

        // 2. Fetch first (separate from merge) so we can compute pulled/pushed
        //    counts from the divergence BEFORE merge creates a merge commit.
        if let Err(e) = repo.fetch_with(&options.remote, &options.branch, creds) {
            let msg = format!("Fetch failed: {e}");
            let _ = logger.error(&msg);
            outcome.errors.push(msg);
            return Ok(finalize_outcome(&cb_dir, outcome));
        }

        let local_tip = repo.rev_parse("HEAD").ok();
        let remote_ref = format!("{}/{}", options.remote, options.branch);
        let remote_tip = repo.rev_parse(&remote_ref).ok();

        // Divergence for reporting. Computed here but assigned to the outcome
        // only after the corresponding step (merge for pulled, push for pushed)
        // actually succeeds, so a later failure never leaves a phantom count.
        let (ahead, behind) = match (local_tip.as_deref(), remote_tip.as_deref()) {
            (Some(local), Some(remote_oid)) => {
                repo.ahead_behind(local, remote_oid).unwrap_or((0, 0))
            }
            // Remote branch does not exist yet: the push below bootstraps it
            // with the local history, so report at least one commit ahead (the
            // count is approximate) rather than a misleading "up to date".
            (Some(_), None) => (1, 0),
            _ => (0, 0),
        };

        // 3. Merge from remote (already fetched). Skip when the remote branch
        //    does not exist yet (never-pushed branch): there is nothing to
        //    merge, and the push below bootstraps refs/heads/<branch>.
        let merge_outcome = if remote_tip.is_some() {
            match repo.merge_fetched(&options.remote, &options.branch) {
                Ok(o) => o,
                Err(e) => {
                    let msg = format!("Merge failed: {e}");
                    let _ = logger.error(&msg);
                    outcome.errors.push(msg);
                    return Ok(finalize_outcome(&cb_dir, outcome));
                }
            }
        } else {
            MergeOutcome::Clean
        };

        match merge_outcome {
            MergeOutcome::Clean => {
                // Accumulate: a push-race retry re-fetches and re-merges, and
                // `behind` then counts only the newly-arrived remote commits.
                outcome.pulled += behind;
            }
            MergeOutcome::Conflicts(conflicted) => match resolver {
                Some(r) => {
                    match resolve_conflicts_inner(&repo, repo_root, &conflicted, r, logger).await {
                        Ok(()) => {
                            outcome.conflicts_resolved += conflicted.len() as u32;
                            repo.finalize_merge_commit_on_branch(None, &options.branch)?;
                            outcome.pulled += behind;
                        }
                        Err(e) => {
                            record_resolver_failure(&repo, &conflicted, &mut outcome, &e, logger)?;
                            return Ok(finalize_outcome(&cb_dir, outcome));
                        }
                    }
                }
                None => {
                    outcome.manual_conflicts += conflicted.len() as u32;
                    let msg = format!(
                        "{} conflict(s) need manual resolution: {}",
                        conflicted.len(),
                        conflicted.join(", ")
                    );
                    let _ = logger.warn(&msg);
                    outcome.errors.push(msg);
                    return Ok(finalize_outcome(&cb_dir, outcome));
                }
            },
        }

        // 4. Push only when configured. Fetch and merge still run in local-only
        //    mode so repositories converge without publishing local commits.
        if !options.auto_push {
            break;
        }
        match repo.push_with(&options.remote, &options.branch, creds) {
            Ok(()) => {
                outcome.pushed = ahead;
                break;
            }
            Err(e) if attempt < MAX_PUSH_RETRIES && is_non_fast_forward_push(&e) => {
                let _ = logger.warn(&format!(
                    "Push failed on attempt {}/{}: {e}. Retrying after re-fetch.",
                    attempt + 1,
                    MAX_PUSH_RETRIES + 1
                ));
                // The next iteration recomputes counts after re-fetch; the
                // outcome fields are only ever set on success, so nothing to reset.
            }
            Err(e) => {
                let msg = format!("Push failed: {e}");
                let _ = logger.error(&msg);
                outcome.errors.push(msg);
                break;
            }
        }
    }

    Ok(finalize_outcome(&cb_dir, outcome))
}

fn is_non_fast_forward_push(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<git2::Error>()
            .is_some_and(|error| error.code() == git2::ErrorCode::NotFastForward)
    })
}

fn record_resolver_failure(
    repo: &GitRepo,
    attempted: &[String],
    outcome: &mut SyncOutcome,
    error: &anyhow::Error,
    logger: &dyn Logger,
) -> Result<()> {
    let remaining = repo.list_conflicted_paths()?;
    let resolved = attempted
        .iter()
        .filter(|path| !remaining.contains(path))
        .count() as u32;
    outcome.conflicts_resolved += resolved;
    outcome.manual_conflicts += remaining.len() as u32;
    let msg = format!(
        "AI resolver failed ({error}); {} conflict(s) still need manual resolution: {}",
        remaining.len(),
        remaining.join(", ")
    );
    let _ = logger.warn(&msg);
    outcome.errors.push(msg);
    Ok(())
}

/// Resolve each conflicted path via the resolver, stage the resolution.
/// Caller invokes `finalize_merge_commit` afterwards.
async fn resolve_conflicts_inner(
    repo: &GitRepo,
    repo_root: &Path,
    paths: &[String],
    resolver: &dyn ConflictResolver,
    logger: &dyn Logger,
) -> Result<()> {
    let _ = logger.info(&format!(
        "Resolving {} conflict(s) via {}",
        paths.len(),
        resolver.name()
    ));

    for path in paths {
        let conflict = repo
            .find_conflict(path)?
            .ok_or_else(|| anyhow::anyhow!("Conflict {path} disappeared from the index"))?;
        if conflict.is_binary_or_special() {
            anyhow::bail!("Conflict {path} is binary or special and requires manual resolution");
        }
        match resolver.resolve(&conflict, repo_root).await? {
            ConflictResolution::WriteContent(content) => {
                repo.resolve_conflict_with_text(path, &content)?;
            }
            ConflictResolution::DeleteFile => {
                repo.resolve_conflict_with_side(path, None)?;
            }
        }
        let _ = logger.info(&format!("Resolved {path}"));
    }

    Ok(())
}

fn finalize_outcome(cb_dir: &Path, outcome: SyncOutcome) -> SyncOutcome {
    let mut state = SyncState::load(cb_dir).unwrap_or_default();
    if outcome.errors.is_empty() {
        state.last_sync_at = Some(crate::utils::datetime::now_iso());
        state.last_error = None;
    } else {
        // Record why the cycle failed; leave last_sync_at pointing at the last
        // successful sync so `commitbook status` distinguishes the two.
        state.last_error = Some(outcome.errors.join("; "));
    }
    let _ = state.save(cb_dir);
    outcome
}

#[cfg(test)]
#[path = "scheduler_tests.rs"]
mod tests;
