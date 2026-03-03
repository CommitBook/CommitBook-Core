use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs;
use std::path::Path;

use crate::ai;
use crate::config::{GlobalConfig, LocalConfig};
use crate::git::GitRepo;
use crate::logger::FileLogger;
use crate::utils::datetime;

/// Run a single auto-commit cycle. Called by cron/launchd.
pub async fn run(repo_path: &Path) -> Result<()> {
    // 1. Acquire file lock to prevent concurrent runs
    let lock_path = LocalConfig::commitbook_dir(repo_path).join(".lock");
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .context("Failed to open lock file")?;

    if lock_file.try_lock_exclusive().is_err() {
        // Another instance is already running
        eprintln!("Another auto-commit is already in progress, skipping.");
        return Ok(());
    }

    // The lock is held until lock_file is dropped at end of scope

    // 2. Load config
    let mut config = LocalConfig::load(repo_path)
        .context("Failed to load CommitBook config")?;

    let logger = FileLogger::new(repo_path, config.logging.max_log_files)?;

    if !config.enabled {
        logger.info("Auto-commit skipped: disabled in config")?;
        return Ok(());
    }

    logger.info("Auto-commit cycle started")?;

    // 3. Open the git repo
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

    logger.info(&format!(
        "Changes detected: {}",
        summary.to_summary_text()
    ))?;

    // 5. Stage all changes
    if let Err(e) = repo.stage_all() {
        logger.error(&format!("Failed to stage changes: {}", e))?;
        return Err(e);
    }
    logger.debug("All changes staged")?;

    // 6. Generate commit message via AI provider chain
    let global = GlobalConfig::load().unwrap_or_default();
    let providers = &global.ai.providers;

    let commit_message = ai::generate_commit_message(&summary, providers, repo_path).await;
    logger.info(&format!("Commit message: {}", commit_message))?;

    // 7. Create the commit
    match repo.commit(&commit_message) {
        Ok(oid) => {
            logger.info(&format!("Commit created: {}", oid))?;
        }
        Err(e) => {
            logger.error(&format!("Failed to create commit: {}", e))?;
            return Err(e);
        }
    }

    // 8. Push to remote if configured
    if config.git.auto_push && repo.has_remote() {
        let remote = repo.default_remote_name().unwrap_or_else(|_| "origin".to_string());
        let branch = config.git.branch.clone();

        match repo.push(&remote, &branch) {
            Ok(()) => {
                logger.info(&format!("Pushed to {}/{}", remote, branch))?;
            }
            Err(e) => {
                logger.error(&format!("Push failed: {} (will retry next cycle)", e))?;
                // Don't return error — the commit is saved locally
            }
        }
    } else if !repo.has_remote() {
        logger.info("No remote configured, skipping push")?;
    }

    // 9. Update last commit timestamp
    config.last_commit = Some(datetime::now_iso());
    if let Err(e) = config.save(repo_path) {
        logger.warn(&format!("Failed to update last_commit timestamp: {}", e))?;
    }

    // 10. Cleanup old logs
    if let Err(e) = logger.cleanup_old_logs() {
        logger.warn(&format!("Failed to cleanup old logs: {}", e))?;
    }

    logger.info("Auto-commit cycle completed successfully")?;

    // Lock is released here when lock_file goes out of scope
    drop(lock_file);
    let _ = fs::remove_file(&lock_path);

    Ok(())
}
