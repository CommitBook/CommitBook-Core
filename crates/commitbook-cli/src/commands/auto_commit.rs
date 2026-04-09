use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs;
use std::path::Path;

use commitbook_core::ai::ProviderChain;
use commitbook_core::config::{GlobalConfig, LocalConfig};
use commitbook_core::git::GitRepo;
use commitbook_core::logger::FileLogger;
use commitbook_core::utils::datetime;

/// Run a single auto-commit cycle. Called by cron/launchd.
pub async fn run(repo_path: &Path) -> Result<()> {
    // 1. Acquire file lock to prevent concurrent runs
    let lock_path = LocalConfig::lock_path(repo_path);
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .context("Failed to open lock file")?;

    if lock_file.try_lock_exclusive().is_err() {
        eprintln!("Another auto-commit is already in progress, skipping.");
        return Ok(());
    }

    // 2. Load config
    let mut config = LocalConfig::load(repo_path)
        .context("Failed to load CommitBook config")?;

    let logger = FileLogger::new(repo_path, config.logging.max_log_files)?;

    if !config.enabled {
        logger.info("Auto-commit skipped: disabled in config")?;
        return Ok(());
    }

    logger.info("Auto-commit cycle started")?;

    // 3. Open git repo
    let repo = match GitRepo::open(repo_path) {
        Ok(r) => r,
        Err(e) => {
            logger.error(&format!("Failed to open git repo: {}", e))?;
            return Err(e);
        }
    };

    // 4. Check for changes
    let summary = match repo.changes_summary() {
        Ok(s) => s,
        Err(e) => {
            logger.error(&format!("Failed to get changes summary: {}", e))?;
            return Err(e);
        }
    };

    if summary.is_empty() {
        logger.info("No changes detected, skipping commit")?;
        return Ok(());
    }

    logger.info(&format!("Changes detected: {}", summary.to_summary_text()))?;

    // 5. Stage all changes
    if let Err(e) = repo.stage_all() {
        logger.error(&format!("Failed to stage changes: {}", e))?;
        return Err(e);
    }

    // 6. Empty diff guard — check for real content changes after staging
    match repo.has_real_staged_changes() {
        Ok(false) => {
            logger.info("No real content changes after staging (mtime-only), skipping")?;
            return Ok(());
        }
        Err(e) => {
            logger.warn(&format!("Could not verify staged changes: {}", e))?;
            // Continue anyway — safer to commit than to skip
        }
        Ok(true) => {}
    }

    // 7. Generate commit message via AI provider chain
    let global = GlobalConfig::load().unwrap_or_default();
    let chain = ProviderChain::new();
    let (commit_message, provider_name) = chain
        .generate(&summary, &global.ai.providers, repo_path)
        .await;

    logger.log_with("INFO", &format!("Commit message: {}", commit_message), Some(&[("provider", &provider_name)]))?;

    // 8. Create the commit
    match repo.commit(&commit_message) {
        Ok(short_hash) => {
            logger.log_with("INFO", &format!("Commit created: {}", short_hash), Some(&[("provider", &provider_name)]))?;
        }
        Err(e) => {
            logger.error(&format!("Failed to create commit: {}", e))?;
            return Err(e);
        }
    }

    // 9. Push to remote if configured
    if config.git.auto_push && repo.has_remote() {
        let remote = repo.default_remote_name().unwrap_or_else(|_| "origin".to_string());
        let branch = config.git.branch.clone();

        // Check for diverged remote first
        match repo.remote_has_diverged(&remote, &branch) {
            Ok(true) => {
                logger.warn("Remote has diverged, attempting pull --rebase")?;
                match repo.pull_rebase(&remote, &branch) {
                    Ok(()) => logger.info("Pull --rebase succeeded")?,
                    Err(e) => {
                        logger.error(&format!("Pull --rebase failed: {}. Manual resolution needed.", e))?;
                        // Don't push — leave it for the user
                        finish_cycle(&mut config, &logger, repo_path)?;
                        return Ok(());
                    }
                }
            }
            Ok(false) => {} // Not diverged, safe to push
            Err(e) => {
                logger.warn(&format!("Could not check remote state: {}", e))?;
            }
        }

        match repo.push(&remote, &branch) {
            Ok(()) => logger.info(&format!("Pushed to {}/{}", remote, branch))?,
            Err(e) => {
                logger.error(&format!("Push failed: {} (will retry next cycle)", e))?;
            }
        }
    } else if !repo.has_remote() {
        logger.info("No remote configured, skipping push")?;
    }

    finish_cycle(&mut config, &logger, repo_path)?;

    // Release lock
    drop(lock_file);
    let _ = fs::remove_file(&lock_path);

    Ok(())
}

fn finish_cycle(config: &mut LocalConfig, logger: &FileLogger, repo_path: &Path) -> Result<()> {
    config.last_commit = Some(datetime::now_iso());
    if let Err(e) = config.save(repo_path) {
        logger.warn(&format!("Failed to update last_commit timestamp: {}", e))?;
    }

    if let Err(e) = logger.cleanup_old_logs() {
        logger.warn(&format!("Failed to cleanup old logs: {}", e))?;
    }

    logger.info("Auto-commit cycle completed")?;
    Ok(())
}
