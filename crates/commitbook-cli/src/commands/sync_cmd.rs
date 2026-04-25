use anyhow::{anyhow, Result};
use colored::Colorize;
use std::path::Path;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::git::GitRepo;
use commitbook_engine::logger::FileLogger;
use commitbook_engine::sync::sync_repository;

/// Run a manual sync: pull-rebase-autostash → optional commit → push.
///
/// Returns an error if the sync surfaced any errors or unresolved manual
/// conflicts so `commitbook sync && next-step` chains correctly.
pub async fn run_sync(cb_dir: &Path, repo_root: &Path) -> Result<()> {
    let config = LocalConfig::load(repo_root)?;
    let logger = FileLogger::new(repo_root, config.logging.max_log_days)?;
    let repo = GitRepo::open(repo_root)?;

    let _ = logger.info("Sync started");

    // Generate the commit message upfront so the engine can use it for the
    // single commit it creates per cycle. Falls back to a per-file default
    // if there are no git-visible changes.
    let commit_message = match repo.changes_summary() {
        Ok(summary) if !summary.is_empty() => {
            Some(generate_commit_message(repo_root, &summary).await)
        }
        _ => None,
    };

    let outcome = sync_repository(cb_dir, repo_root, &config, &logger, commit_message).await;

    let exit_err: Option<anyhow::Error> = match outcome {
        Ok(o) => {
            if o.pulled > 0 {
                println!("  {} Pulled {} commit(s).", "OK".green().bold(), o.pulled);
            }
            if o.pushed > 0 {
                println!("  {} Pushed {} commit(s).", "OK".green().bold(), o.pushed);
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
                    "  {} {} conflict(s) need manual resolution. Run `git status` to see them.",
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
pub async fn run_scheduled(cb_dir: &Path, repo_root: &Path) -> Result<()> {
    use fs2::FileExt;

    let lock_path = LocalConfig::lock_path(repo_root);
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)?;

    if lock_file.try_lock_exclusive().is_err() {
        // Another cycle is running.
        return Ok(());
    }

    let config = LocalConfig::load(repo_root)?;
    if !config.enabled {
        return Ok(());
    }

    let logger = FileLogger::new(repo_root, config.logging.max_log_days)?;
    let _ = logger.info("Scheduled sync cycle started");
    log::info!("Starting scheduled sync cycle");

    if let Err(e) = run_sync(cb_dir, repo_root).await {
        let _ = logger.error(&format!("Scheduled sync failed: {e}"));
        log::error!("Scheduled sync failed: {e}");
    }

    let _ = logger.info("Scheduled sync cycle complete");
    log::info!("Scheduled sync cycle complete");

    let _ = lock_file.unlock();
    let _ = std::fs::remove_file(&lock_path);

    Ok(())
}

/// Generate a commit message using AI or fallback.
async fn generate_commit_message(
    repo_root: &Path,
    summary: &commitbook_engine::git::ChangesSummary,
) -> String {
    let chain = commitbook_engine::ai::ProviderChain::new();
    let keys = vec![
        "gh-copilot".to_string(),
        "claude-cli".to_string(),
        "codex-cli".to_string(),
        "fallback".to_string(),
    ];
    let (msg, _provider) = chain.generate(summary, &keys, repo_root).await;
    msg
}

