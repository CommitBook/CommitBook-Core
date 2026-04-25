//! Pull-first, rebase-style sync orchestrator.
//!
//! Executes one cycle of `git pull --rebase --autostash` → AI conflict
//! resolution (if configured) → commit dirty markdown → `git push`. Retries
//! once on a non-fast-forward push (race with another client).
//!
//! Replaces the legacy planner / pipeline / transport stack. Remains opt-in
//! via `[sync] engine = "v2"` until validated.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::ai::{ConflictResolver, ResolverRegistry};
use crate::config::LocalConfig;
use crate::git::operations::PullOutcome;
use crate::git::GitRepo;
use crate::platform::Logger;
use crate::state::sync_state::SyncState;

const MAX_PUSH_RETRIES: u32 = 1;

/// Result of one v2 sync cycle.
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
/// Loads the configured conflict resolver from the registry; for tests
/// or programmatic callers wanting to inject a fake resolver, use
/// `sync_with_resolver` directly.
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
    logger: &dyn Logger,
    commit_message: Option<String>,
) -> Result<SyncOutcome> {
    let repo = GitRepo::open(repo_root)?;
    let mut outcome = SyncOutcome::default();

    for attempt in 0..=MAX_PUSH_RETRIES {
        let head_before_pull = repo.rev_parse("HEAD").ok();

        // 1. Pull-rebase-autostash
        match repo.pull_rebase_autostash(remote, branch) {
            Ok(PullOutcome::Clean) => {}
            Ok(PullOutcome::StashPopConflict) => {
                let conflicted = repo.list_conflicted_paths()?;
                if conflicted.is_empty() {
                    let _ = logger.warn("Stash-pop conflict reported but no unmerged paths");
                } else {
                    match resolver {
                        Some(r) => {
                            match resolve_conflicts_inner(repo_root, &conflicted, r, logger).await {
                                Ok(()) => {
                                    outcome.conflicts_resolved += conflicted.len() as u32;
                                    repo.continue_rebase_or_stash()?;
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
            Ok(PullOutcome::RebaseConflict) => {
                let _ = repo.rebase_abort();
                let msg = format!(
                    "Local commits conflict with `{remote}/{branch}` during rebase — \
                     resolve manually and re-run sync."
                );
                let _ = logger.error(&msg);
                outcome.errors.push(msg);
                return Ok(finalize_outcome(cb_dir, outcome));
            }
            Err(e) => {
                let msg = format!("Pull failed: {e}");
                let _ = logger.error(&msg);
                outcome.errors.push(msg);
                return Ok(finalize_outcome(cb_dir, outcome));
            }
        }

        let head_after_pull = repo.rev_parse("HEAD").ok();
        outcome.pulled =
            commits_between(repo_root, head_before_pull.as_deref(), head_after_pull.as_deref())?;

        // 2. Commit dirty markdown
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

        // 3. Push
        let head_before_push = repo.rev_parse("HEAD").ok();
        match repo.push(remote, branch) {
            Ok(()) => {
                outcome.pushed = commits_between(
                    repo_root,
                    head_after_pull.as_deref(),
                    head_before_push.as_deref(),
                )?;
                break;
            }
            Err(e) if attempt < MAX_PUSH_RETRIES => {
                let _ = logger.warn(&format!(
                    "Push failed on attempt {}/{}: {e}. Retrying after re-pull.",
                    attempt + 1,
                    MAX_PUSH_RETRIES + 1
                ));
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

async fn resolve_conflicts_inner(
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
        std::fs::write(&abs_path, resolved)
            .with_context(|| format!("Failed to write {}", abs_path.display()))?;

        let add = Command::new("git")
            .args(["add", path])
            .current_dir(repo_root)
            .output()
            .context("Failed to run git add")?;
        if !add.status.success() {
            anyhow::bail!(
                "git add {path} failed: {}",
                String::from_utf8_lossy(&add.stderr).trim()
            );
        }
        let _ = logger.info(&format!("Resolved {path}"));
    }

    Ok(())
}

fn commits_between(
    repo_root: &Path,
    older: Option<&str>,
    newer: Option<&str>,
) -> Result<u32> {
    match (older, newer) {
        (Some(o), Some(n)) if o != n => {
            let output = Command::new("git")
                .args(["rev-list", "--count", &format!("{o}..{n}")])
                .current_dir(repo_root)
                .output()
                .context("Failed to run git rev-list")?;
            if !output.status.success() {
                return Ok(0);
            }
            Ok(String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .unwrap_or(0))
        }
        _ => Ok(0),
    }
}

fn finalize_outcome(cb_dir: &Path, outcome: SyncOutcome) -> SyncOutcome {
    let mut state = SyncState::load(cb_dir).unwrap_or_default();
    state.last_sync_at = Some(crate::utils::datetime::now_iso());
    let _ = state.save(cb_dir);
    outcome
}

#[cfg(test)]
#[path = "scheduler_tests.rs"]
mod tests;
