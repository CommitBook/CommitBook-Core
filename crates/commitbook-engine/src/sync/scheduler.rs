//! Merge-based sync orchestrator over libgit2.
//!
//! Executes one cycle of: commit dirty non-ignored changes → fetch + merge →
//! AI conflict resolution (if configured) or surface conflicts to caller →
//! optionally push (retry once on race). Single code path on desktop and mobile,
//! see `docs/details/sync-architecture.md`.

use anyhow::Result;
use std::path::Path;

use crate::ai::{ConflictResolution, ConflictResolver, ResolverRegistry};
use crate::config::{local::GitSettings, ConflictMode, LocalConfig};
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
    pub review_ai_resolutions: bool,
    /// `both` conflict mode: keep both versions of conflicting notes.
    pub keep_both: bool,
}

impl SyncOptions {
    pub fn new(remote: impl Into<String>, branch: impl Into<String>, auto_push: bool) -> Self {
        Self {
            remote: remote.into(),
            branch: branch.into(),
            auto_push,
            review_ai_resolutions: false,
            keep_both: false,
        }
    }
}

impl From<&GitSettings> for SyncOptions {
    fn from(settings: &GitSettings) -> Self {
        Self {
            remote: settings.remote.clone(),
            branch: settings.branch.clone(),
            // Sync always publishes; `auto_push` remains for tests and a
            // possible future per-device setting.
            auto_push: true,
            review_ai_resolutions: false,
            keep_both: false,
        }
    }
}

/// Result of one sync cycle.
#[derive(Debug, Default, serde::Serialize)]
pub struct SyncOutcome {
    pub committed: bool,
    pub pushed: u32,
    pub pulled: u32,
    pub conflicts_resolved: u32,
    /// Notes whose conflicts were resolved by keeping both versions
    /// (`both` mode); the user deletes the version they don't want.
    pub kept_both: Vec<String>,
    pub manual_conflicts: u32,
    pub errors: Vec<String>,
}

impl SyncOutcome {
    pub fn is_clean(&self) -> bool {
        !self.committed
            && self.pushed == 0
            && self.pulled == 0
            && self.conflicts_resolved == 0
            && self.kept_both.is_empty()
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
    // Safety net for clones that never ran `init` on this device: register
    // it with the default name so other devices can see it.
    if crate::devices::this_device_id(repo_root)?.is_none() {
        let auth = crate::devices::desktop_auth(repo_root, &config.git.remote);
        crate::devices::register(repo_root, None, auth)?;
    }
    let registry = ResolverRegistry::new();
    let resolver = match config.conflicts.mode {
        ConflictMode::Both | ConflictMode::Manual => None,
        ConflictMode::Ai | ConflictMode::Review => registry.get(config.conflicts.agent.as_str()),
    };
    let mut options = SyncOptions::from(&config.git);
    options.review_ai_resolutions = config.conflicts.mode == ConflictMode::Review;
    options.keep_both = config.conflicts.mode == ConflictMode::Both;
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
    let cb_dir = LocalConfig::commitbook_dir(repo_root);
    // Never replace malformed state with defaults; validate before mutations.
    let mut state = SyncState::load(&cb_dir)?;
    state.last_attempt_at = Some(crate::utils::datetime::now_iso());
    state.save(&cb_dir)?;
    let mut stage = "inspection";
    let result = sync_cycle(
        repo_root,
        options,
        resolver,
        creds,
        logger,
        commit_message,
        lock,
        &mut state,
        &mut stage,
    )
    .await;
    // Publication cleanup may update pending_init_push during the cycle.
    let persist = (|| -> Result<()> {
        let mut current = SyncState::load(&cb_dir)?;
        current.last_attempt_at = state.last_attempt_at;
        current.last_fetch_at = state.last_fetch_at;
        current.last_push_at = state.last_push_at;
        let failure = match &result {
            Ok(outcome) if !outcome.errors.is_empty() => Some(outcome.errors.join("; ")),
            Err(error) => Some(format!("{error:#}")),
            _ => None,
        };
        current.last_error_stage = failure.as_ref().map(|_| stage.to_string());
        if let Ok(outcome) = &result {
            if !outcome.kept_both.is_empty() {
                current.kept_both_paths = outcome.kept_both.clone();
                current.kept_both_at = Some(crate::utils::datetime::now_iso());
            }
        }
        current.last_error = failure;
        if let Ok(outcome) = &result {
            if outcome.errors.is_empty() && outcome.manual_conflicts == 0 {
                current.last_sync_at = Some(crate::utils::datetime::now_iso());
            }
        }
        current.save(&cb_dir)
    })();
    match (result, persist) {
        (Ok(outcome), Ok(())) => Ok(outcome),
        (Err(error), Ok(())) => Err(error),
        (Ok(outcome), Err(error)) => anyhow::bail!(
            "Sync state persistence failed: {error:#}; sync errors: {}",
            outcome.errors.join("; ")
        ),
        (Err(error), Err(save_error)) => {
            anyhow::bail!("{error:#}; sync state persistence also failed: {save_error:#}")
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn sync_cycle(
    repo_root: &Path,
    options: &SyncOptions,
    resolver: Option<&dyn ConflictResolver>,
    creds: &dyn CredentialProvider,
    logger: &dyn Logger,
    commit_message: Option<String>,
    lock: &RepoLock,
    state: &mut SyncState,
    stage: &mut &'static str,
) -> Result<SyncOutcome> {
    let repo = GitRepo::open(repo_root)?;
    let current_branch = repo.current_branch()?;
    if current_branch != options.branch {
        anyhow::bail!(
            "Cannot sync while checked out on branch {:?}: CommitBook is configured for {:?}. Check out the configured branch and retry.",
            current_branch,
            options.branch
        );
    }
    crate::review::cleanup_locked(repo_root, lock)?;
    let mut outcome = SyncOutcome::default();

    for attempt in 0..=MAX_PUSH_RETRIES {
        // 0. Recover from a merge left in progress by a previous manual-mode
        //    or failed-resolver cycle before touching the working tree.
        //    Guarding on merge_in_progress() makes this a no-op on retries.
        *stage = "merge";
        if repo.merge_in_progress() {
            let unresolved = keep_both_notes(
                &repo,
                repo.list_conflicted_paths()?,
                options.keep_both,
                &mut outcome,
                logger,
            );
            if !unresolved.is_empty() {
                *stage = "ai_resolution";
                if crate::review::prepare_locked(
                    repo_root,
                    options.review_ai_resolutions,
                    resolver,
                    lock,
                )
                .await?
                {
                    outcome.manual_conflicts = unresolved.len() as u32;
                    return Ok(outcome);
                }
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
                            return Ok(outcome);
                        }
                        *stage = "merge";
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
                        return Ok(outcome);
                    }
                }
            } else {
                // Markers already resolved by the user or the keep-both
                // pass: complete the merge.
                repo.finalize_merge_commit_on_branch(None, &options.branch)?;
            }
        }

        // 1. Commit every dirty, non-ignored Git change FIRST. Doing this
        //    before the merge means
        //    a) we never lose work to a failed merge, b) the merge sees a
        //    proper local commit if the dirty file overlaps with remote.
        *stage = "staging";
        if repo.has_dirty_changes()? {
            repo.stage_all()?;
            if repo.has_real_staged_changes()? {
                let msg = commit_message.clone().unwrap_or_else(|| {
                    crate::ai::fallback::generate_timestamp_message(&Default::default())
                });
                *stage = "commit";
                repo.commit_on_branch(&msg, &options.branch)?;
                outcome.committed = true;
            }
        }

        // 2. Fetch first (separate from merge) so we can compute pulled/pushed
        //    counts from the divergence BEFORE merge creates a merge commit.
        *stage = "fetch";
        if let Err(e) = repo.fetch_with(&options.remote, &options.branch, creds) {
            let msg = format!("Fetch failed: {e}");
            let _ = logger.error(&msg);
            outcome.errors.push(msg);
            return Ok(outcome);
        }

        state.last_fetch_at = Some(crate::utils::datetime::now_iso());
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
        *stage = "merge";
        let merge_outcome = if remote_tip.is_some() {
            match repo.merge_fetched(&options.remote, &options.branch) {
                Ok(o) => o,
                Err(e) => {
                    let msg = format!("Merge failed: {e}");
                    let _ = logger.error(&msg);
                    outcome.errors.push(msg);
                    return Ok(outcome);
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
                let conflicted =
                    keep_both_notes(&repo, conflicted, options.keep_both, &mut outcome, logger);
                if conflicted.is_empty() {
                    repo.finalize_merge_commit_on_branch(None, &options.branch)?;
                    outcome.pulled += behind;
                    // Fall through to push below.
                } else {
                    *stage = "ai_resolution";
                    if crate::review::prepare_locked(
                        repo_root,
                        options.review_ai_resolutions,
                        resolver,
                        lock,
                    )
                    .await?
                    {
                        outcome.manual_conflicts = conflicted.len() as u32;
                        return Ok(outcome);
                    }
                    match resolver {
                        Some(r) => {
                            match resolve_conflicts_inner(&repo, repo_root, &conflicted, r, logger)
                                .await
                            {
                                Ok(()) => {
                                    outcome.conflicts_resolved += conflicted.len() as u32;
                                    *stage = "merge";
                                    repo.finalize_merge_commit_on_branch(None, &options.branch)?;
                                    outcome.pulled += behind;
                                }
                                Err(e) => {
                                    record_resolver_failure(
                                        &repo,
                                        &conflicted,
                                        &mut outcome,
                                        &e,
                                        logger,
                                    )?;
                                    return Ok(outcome);
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
                            return Ok(outcome);
                        }
                    }
                }
            }
        }

        // 4. Push only when configured. Fetch and merge still run in local-only
        //    mode so repositories converge without publishing local commits.
        if !options.auto_push {
            break;
        }
        *stage = "push";
        let published_tip = repo.rev_parse("HEAD")?;
        match repo.push_commit_with(&options.remote, &options.branch, &published_tip, creds) {
            Ok(()) => {
                state.last_push_at = Some(crate::utils::datetime::now_iso());
                outcome.pushed = ahead;
                if let Err(error) = crate::commitbooks::publication::clear_published(
                    &repo,
                    &options.remote,
                    &options.branch,
                    &published_tip,
                ) {
                    outcome.errors.push(format!(
                        "Push succeeded but initialization state cleanup failed: {error:#}"
                    ));
                }
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

    Ok(outcome)
}

fn is_non_fast_forward_push(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<git2::Error>()
            .is_some_and(|error| error.code() == git2::ErrorCode::NotFastForward)
    })
}

/// Notes eligible for `both` mode. Structured files such as JSON or YAML
/// would be corrupted by keeping two versions, so they fall back to manual.
fn is_note_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".md", ".markdown", ".txt"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// In `both` mode, resolve conflicting notes by keeping both versions
/// without markers (see `GitRepo::try_resolve_both`). Returns the paths that
/// still need the configured resolver or the user. A failure on one path
/// leaves it for those later steps instead of failing the cycle.
fn keep_both_notes(
    repo: &GitRepo,
    paths: Vec<String>,
    enabled: bool,
    outcome: &mut SyncOutcome,
    logger: &dyn Logger,
) -> Vec<String> {
    if !enabled {
        return paths;
    }
    let mut remaining = Vec::new();
    for path in paths {
        if !is_note_path(&path) {
            remaining.push(path);
            continue;
        }
        match repo.try_resolve_both(&path) {
            Ok(true) => {
                let _ = logger.warn(&format!(
                    "Kept both versions in {path}; delete the one you don't want"
                ));
                outcome.kept_both.push(path);
            }
            Ok(false) => remaining.push(path),
            Err(error) => {
                let _ = logger.warn(&format!(
                    "Keeping both versions of {path} failed ({error:#}); leaving it for manual resolution"
                ));
                remaining.push(path);
            }
        }
    }
    remaining
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

#[cfg(test)]
#[path = "scheduler_tests.rs"]
mod tests;
