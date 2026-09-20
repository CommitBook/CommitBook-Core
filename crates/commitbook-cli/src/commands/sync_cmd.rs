use anyhow::{anyhow, Result};
use colored::Colorize;
use std::path::Path;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::git::GitRepo;
use commitbook_engine::logger::FileLogger;
use commitbook_engine::state::{RepoLock, RepoLockContended};
use commitbook_engine::sync::sync_repository_locked;

/// Run a manual sync: commit dirty unignored changes, fetch, merge, then
/// optionally push (retrying once on a non-fast-forward race).
///
/// Returns an error if the sync surfaced any errors or unresolved manual
/// conflicts so `commitbook sync && next-step` chains correctly.
pub async fn run_sync(repo_root: &Path) -> Result<()> {
    let lock = RepoLock::acquire(repo_root)?;
    run_sync_locked(repo_root, &lock).await
}

async fn run_sync_locked(repo_root: &Path, lock: &RepoLock) -> Result<()> {
    let config = LocalConfig::load(repo_root)?;
    let logger = FileLogger::new(repo_root, config.logging.max_log_days)?;
    let repo = GitRepo::open(repo_root)?;

    let _ = logger.info("Sync started");

    // Generate the commit message upfront so the engine can use it for the
    // single commit it creates per cycle. Gate on the same predicate the engine
    // commits on (any dirty non-ignored Git change) so we never spawn an AI CLI when the engine
    // will not commit.
    let commit_message = if repo.has_dirty_changes().unwrap_or(false) {
        let summary = repo.changes_summary().unwrap_or_default();
        Some(generate_commit_message(repo_root, &summary, config.commit.ai_messages).await)
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
            let _ = logger.error(&format!("Sync failed: {e}"));
            println!("  {} Sync failed: {}", "ERROR".red().bold(), e);
            println!(
                "{}",
                "  Working tree preserved. Will retry on next sync.".dimmed()
            );
            Some(e)
        }
    };

    let _ = logger.cleanup_old_logs();
    match exit_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Run a scheduled sync cycle (hidden `commitbook run` command).
pub async fn run_scheduled(repo_root: &Path) -> Result<()> {
    let lock = match RepoLock::acquire(repo_root) {
        Ok(lock) => lock,
        Err(error) if error.downcast_ref::<RepoLockContended>().is_some() => {
            log::info!("Scheduled sync skipped because another operation is running");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let config = LocalConfig::load(repo_root)?;
    if !config.enabled {
        return Ok(());
    }

    let logger = FileLogger::new(repo_root, config.logging.max_log_days)?;
    let _ = logger.info("Scheduled sync cycle started");
    log::info!("Starting scheduled sync cycle");

    if let Err(e) = run_sync_locked(repo_root, &lock).await {
        let _ = logger.error(&format!("Scheduled sync failed: {e}"));
        log::error!("Scheduled sync failed: {e}");
    }

    let _ = logger.info("Scheduled sync cycle complete");
    log::info!("Scheduled sync cycle complete");

    Ok(())
}

/// Provider keys to try, in order, for a commit message.
///
/// When `ai_messages` is false the AI CLIs are skipped entirely and only the
/// deterministic timestamp `fallback` provider is used.
fn commit_provider_keys(ai_messages: bool) -> Vec<String> {
    if ai_messages {
        vec![
            "gh-copilot".to_string(),
            "claude-cli".to_string(),
            "codex-cli".to_string(),
            "fallback".to_string(),
        ]
    } else {
        vec!["fallback".to_string()]
    }
}

/// Generate a commit message using AI or fallback.
async fn generate_commit_message(
    repo_root: &Path,
    summary: &commitbook_engine::git::ChangesSummary,
    ai_messages: bool,
) -> String {
    generate_commit_message_with_chain(
        repo_root,
        summary,
        ai_messages,
        commitbook_engine::ai::ProviderChain::new,
    )
    .await
}

async fn generate_commit_message_with_chain(
    repo_root: &Path,
    summary: &commitbook_engine::git::ChangesSummary,
    ai_messages: bool,
    make_chain: impl FnOnce() -> commitbook_engine::ai::ProviderChain,
) -> String {
    if !ai_messages {
        return commitbook_engine::ai::fallback::generate_timestamp_message(summary);
    }
    let chain = make_chain();
    let keys = commit_provider_keys(ai_messages);
    let (msg, _provider) = chain.generate(summary, &keys, repo_root).await;
    msg
}

#[cfg(test)]
#[path = "sync_cmd_tests.rs"]
mod tests;
