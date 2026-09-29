use anyhow::{anyhow, Result};
use colored::Colorize;
use std::path::Path;

use commitbook_engine::config::{CommitAgent, CommitMode, LocalConfig};
use commitbook_engine::git::GitRepo;
use commitbook_engine::logger::FileLogger;
use commitbook_engine::state::{RepoLock, RepoLockContended};
use commitbook_engine::sync::sync_repository_locked;

/// Run one sync cycle: commit dirty unignored changes, fetch, merge, then
/// push (retrying once on a non-fast-forward race). The scheduler runs this
/// same command.
///
/// Returns an error if the sync surfaced any errors or unresolved manual
/// conflicts, or if another operation holds the repository lock, so
/// `commitbook sync && next-step` chains correctly and a failed scheduled run
/// exits non-zero. A failed cycle is printed here and returned as
/// `errors::Reported`, so it appears once; `verbose` prints the full chain.
pub async fn run_sync(repo_root: &Path, verbose: bool) -> Result<()> {
    let lock = match RepoLock::acquire(repo_root) {
        Ok(lock) => lock,
        Err(error) => {
            if error.downcast_ref::<RepoLockContended>().is_some() {
                log_skipped_sync(repo_root);
            }
            return Err(error);
        }
    };
    run_sync_locked(repo_root, &lock, verbose).await
}

/// Record a sync skipped because the lock was taken, so skipped scheduled
/// runs show up in `commitbook log`. Best effort: never masks the lock error.
fn log_skipped_sync(repo_root: &Path) {
    let keep = LocalConfig::load(repo_root)
        .map(|config| config.logs.keep)
        .unwrap_or_default();
    if let Ok(logger) = FileLogger::new(repo_root, keep) {
        let _ = logger.warn("Sync skipped: another operation is running");
    }
}

/// Record a config that fails to load, before any logger settings are known:
/// in the daily log (default retention) and as the last error in
/// `state.toml`, so `commitbook log` and `status` explain a failing
/// scheduled sync. Best effort: never masks the load error.
fn record_config_failure(repo_root: &Path, error: &anyhow::Error) {
    let message = format!("Sync failed: cannot load .CommitBook/config.toml: {error:#}");
    if let Ok(logger) = FileLogger::new(repo_root, Default::default()) {
        let _ = logger.error(&message);
    }
    let cb_dir = LocalConfig::commitbook_dir(repo_root);
    if let Ok(mut state) = commitbook_engine::state::sync_state::SyncState::load(&cb_dir) {
        state.last_attempt_at = Some(commitbook_engine::utils::datetime::now_iso());
        state.last_error = Some(message);
        state.last_error_stage = Some("config".to_string());
        let _ = state.save(&cb_dir);
    }
}

async fn run_sync_locked(repo_root: &Path, lock: &RepoLock, verbose: bool) -> Result<()> {
    let config = match LocalConfig::load(repo_root) {
        Ok(config) => config,
        Err(error) => {
            record_config_failure(repo_root, &error);
            return Err(error);
        }
    };
    let logger = FileLogger::new(repo_root, config.logs.keep)?;
    let repo = GitRepo::open(repo_root)?;

    let _ = logger.info("Sync started");

    // Generate the commit message upfront so the engine can use it for the
    // single commit it creates per cycle. Gate on the same predicate the engine
    // commits on (any dirty non-ignored Git change) so we never spawn an AI CLI when the engine
    // will not commit.
    let commit_message = if repo.has_dirty_changes().unwrap_or(false) {
        let summary = repo.changes_summary().unwrap_or_default();
        Some(
            generate_commit_message(repo_root, &summary, config.commit.mode, config.commit.agent)
                .await,
        )
    } else {
        None
    };

    let outcome = sync_repository_locked(repo_root, &config, &logger, commit_message, lock).await;

    let exit_err: Option<anyhow::Error> = match outcome {
        Ok(o) => {
            // Only claim success counts when the cycle had no errors.
            if o.errors.is_empty() {
                if o.pulled > 0 {
                    println!("  {} Pulled {} commit(s).", "OK".green().bold(), o.pulled);
                }
                if o.pushed > 0 {
                    println!("  {} Pushed {} commit(s).", "OK".green().bold(), o.pushed);
                }
            }
            if !o.kept_both.is_empty() {
                println!(
                    "  {} Kept both versions in {}; delete the one you don't want.",
                    "WARN".yellow().bold(),
                    o.kept_both.join(", ")
                );
            }
            if o.conflicts_resolved > 0 {
                println!(
                    "  {} Resolved {} conflict(s) via AI.",
                    "OK".green().bold(),
                    o.conflicts_resolved
                );
            }
            if o.manual_conflicts > 0 {
                println!(
                    "  {} {} conflict(s) need manual resolution. Open the web dashboard /conflicts to review or resolve them; `git status` also lists them.",
                    "WARN".yellow().bold(),
                    o.manual_conflicts
                );
            }
            for err in &o.errors {
                println!("  {} {}", "ERROR".red().bold(), err);
            }
            if o.is_clean() {
                println!("{}", "Already up to date.".dimmed());
            }
            if o.errors.is_empty() && o.manual_conflicts == 0 {
                None
            } else {
                Some(anyhow!(
                    "sync completed with {} error(s) and {} unresolved conflict(s)",
                    o.errors.len(),
                    o.manual_conflicts
                ))
            }
        }
        Err(e) if e.downcast_ref::<RepoLockContended>().is_some() => Some(e),
        Err(e) => {
            let _ = logger.error(&format!("Sync failed: {e:#}"));
            let message = if verbose {
                format!("{e:#}")
            } else {
                crate::errors::humanize(&e)
            };
            // The returned error is `Reported`, so these stderr lines are the
            // only place the failure is shown; callers read diagnostics there.
            eprintln!("  {} Sync failed: {message}", "ERROR".red().bold());
            eprintln!("{}", "  Working tree preserved.".dimmed());
            if !verbose && crate::errors::verbose_adds_detail(&e) {
                eprintln!("{}", "  Run with --verbose for the full error.".dimmed());
            }
            Some(crate::errors::Reported(e).into())
        }
    };

    let _ = logger.info("Sync complete");
    let _ = logger.cleanup_old_logs();
    match exit_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Generate a commit message using AI or fallback.
async fn generate_commit_message(
    repo_root: &Path,
    summary: &commitbook_engine::git::ChangesSummary,
    mode: CommitMode,
    agent: CommitAgent,
) -> String {
    generate_commit_message_with_chain(
        repo_root,
        summary,
        mode,
        agent,
        commitbook_engine::ai::ProviderChain::new,
    )
    .await
}

async fn generate_commit_message_with_chain(
    repo_root: &Path,
    summary: &commitbook_engine::git::ChangesSummary,
    mode: CommitMode,
    agent: CommitAgent,
    make_chain: impl FnOnce() -> commitbook_engine::ai::ProviderChain,
) -> String {
    if mode == CommitMode::Timestamp {
        return commitbook_engine::ai::fallback::generate_timestamp_message(summary);
    }
    let chain = make_chain();
    let keys = commitbook_engine::ai::commit_provider_keys(mode, agent);
    let (msg, _provider) = chain.generate(summary, &keys, repo_root).await;
    msg
}

#[cfg(test)]
#[path = "sync_cmd_tests.rs"]
mod tests;
