//! Merge-based sync orchestrator over libgit2.
//!
//! Executes one cycle of: commit dirty markdown → fetch + 3-way merge →
//! AI conflict resolution (if configured) or surface conflicts to caller →
//! push (retry once on race). Single code path on desktop and mobile ,
//! see `docs/details/sync-architecture.md`.

use anyhow::{Context, Result};
use std::path::Path;

use crate::ai::{ConflictResolver, ResolverRegistry};
use crate::config::LocalConfig;
use crate::git::operations::MergeOutcome;
use crate::git::GitRepo;
use crate::platform::{CredentialProvider, Logger, SystemCredentials};
use crate::state::sync_state::SyncState;

const MAX_PUSH_RETRIES: u32 = 1;

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
    cb_dir: &Path,
    repo_root: &Path,
    config: &LocalConfig,
    logger: &dyn Logger,
    commit_message: Option<String>,
) -> Result<SyncOutcome> {
    let registry = ResolverRegistry::new();
    let resolver = registry.get(&config.conflict.resolver);
    sync_with_resolver(
        cb_dir,
        repo_root,
        &config.git.remote,
        &config.git.branch,
        resolver,
        &SystemCredentials,
        logger,
        commit_message,
    )
    .await
}

/// Lower-level entry that accepts an explicit resolver (or `None` for
/// manual mode). Used by `sync_repository` and by tests.
pub async fn sync_with_resolver(
    cb_dir: &Path,
    repo_root: &Path,
    remote: &str,
    branch: &str,
    resolver: Option<&dyn ConflictResolver>,
    creds: &dyn CredentialProvider,
    logger: &dyn Logger,
    commit_message: Option<String>,
) -> Result<SyncOutcome> {
    let repo = GitRepo::open(repo_root)?;
    let mut outcome = SyncOutcome::default();

    for attempt in 0..=MAX_PUSH_RETRIES {
        // 0. Recover from a merge left in progress by a previous manual-mode
        //    or failed-resolver cycle before touching the working tree.
        //    Guarding on merge_in_progress() makes this a no-op on retries.
        if repo.merge_in_progress() {
            let unresolved = repo.list_conflicted_paths()?;
            if !unresolved.is_empty() {
                outcome.manual_conflicts += unresolved.len() as u32;
                let msg = format!(
                    "{} conflict(s) still need manual resolution: {}",
                    unresolved.len(),
                    unresolved.join(", ")
                );
                let _ = logger.warn(&msg);
                outcome.errors.push(msg);
                return Ok(finalize_outcome(cb_dir, outcome));
            }
            // Markers already resolved by the user: complete the merge.
            repo.finalize_merge_commit(None)?;
        }

        // 1. Commit dirty markdown FIRST. Doing this before the merge means
        //    a) we never lose work to a failed merge, b) the merge sees a
        //    proper local commit if the dirty file overlaps with remote.
        if repo.has_dirty_markdown()? {
            repo.stage_all()?;
            if repo.has_real_staged_changes()? {
                let msg = commit_message.clone().unwrap_or_else(|| {
                    format!("Update via CommitBook ({})", crate::utils::datetime::now_iso())
                });
                repo.commit(&msg)?;
                outcome.committed = true;
            }
        }

        // 2. Fetch first (separate from merge) so we can compute pulled/pushed
        //    counts from the divergence BEFORE merge creates a merge commit.
        if let Err(e) = repo.fetch_with(remote, branch, creds) {
            let msg = format!("Fetch failed: {e}");
            let _ = logger.error(&msg);
            outcome.errors.push(msg);
            return Ok(finalize_outcome(cb_dir, outcome));
        }

        let local_tip = repo.rev_parse("HEAD").ok();
        let remote_ref = format!("{remote}/{branch}");
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
            match repo.merge_fetched(remote, branch) {
                Ok(o) => o,
                Err(e) => {
                    let msg = format!("Merge failed: {e}");
                    let _ = logger.error(&msg);
                    outcome.errors.push(msg);
                    return Ok(finalize_outcome(cb_dir, outcome));
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
            MergeOutcome::Conflicts(conflicted) => {
                match resolver {
                    Some(r) => {
                        match resolve_conflicts_inner(&repo, repo_root, &conflicted, r, logger).await {
                            Ok(()) => {
                                outcome.conflicts_resolved += conflicted.len() as u32;
                                repo.finalize_merge_commit(None)?;
                                outcome.pulled += behind;
                            }
                            Err(e) => {
                                outcome.manual_conflicts += conflicted.len() as u32;
                                let msg = format!(
                                    "AI resolver failed ({}); {} conflict(s) need manual resolution: {}",
                                    e,
                                    conflicted.len(),
                                    conflicted.join(", ")
                                );
                                let _ = logger.warn(&msg);
                                outcome.errors.push(msg);
                                return Ok(finalize_outcome(cb_dir, outcome));
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
                        return Ok(finalize_outcome(cb_dir, outcome));
                    }
                }
            }
        }

        // 4. Push.
        match repo.push_with(remote, branch, creds) {
            Ok(()) => {
                outcome.pushed = ahead;
                break;
            }
            Err(e) if attempt < MAX_PUSH_RETRIES => {
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

    Ok(finalize_outcome(cb_dir, outcome))
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
        let abs_path = repo_root.join(path);
        let content = std::fs::read_to_string(&abs_path)
            .with_context(|| format!("Failed to read {}", abs_path.display()))?;
        let resolved = resolver.resolve(Path::new(path), &content, repo_root).await?;
        // A resolver that returns unresolved markers must not have its output
        // committed; fall back to manual resolution instead.
        if resolved.contains("<<<<<<<") {
            anyhow::bail!("Resolver left conflict markers in {path}");
        }
        std::fs::write(&abs_path, resolved)
            .with_context(|| format!("Failed to write {}", abs_path.display()))?;

        repo.stage_paths(std::slice::from_ref(path))
            .with_context(|| format!("Failed to stage resolved {path}"))?;
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
