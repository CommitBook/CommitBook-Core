use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_core::config::LocalConfig;
use commitbook_core::git::GitRepo;
use commitbook_core::logger::FileLogger;
use commitbook_core::sync::scheduler::sync_repository;
use commitbook_core::transport::git_remote::GitRemoteTransport;

/// Run a manual sync: one commit covering dirty tracked files, then pull/merge/push.
pub async fn run_sync(cb_dir: &Path, repo_root: &Path) -> Result<()> {
    let config = LocalConfig::load(repo_root)?;
    let logger = FileLogger::new(repo_root, config.logging.max_log_days)?;
    let repo = GitRepo::open(repo_root)?;

    let _ = logger.info("Sync started");

    // Generate the commit message upfront so the pipeline can use it for the
    // single commit it creates per cycle. Falls back to a per-file default if
    // there are no git-visible changes.
    let commit_message = match repo.changes_summary() {
        Ok(summary) if !summary.is_empty() => {
            Some(generate_commit_message(repo_root, &summary).await)
        }
        _ => None,
    };

    let transport = create_transport(cb_dir, repo_root, &config)?;
    let tracked_patterns: Vec<String> = config.files.include.clone();

    match sync_repository(
        cb_dir,
        repo_root,
        &config.git.remote,
        &config.git.branch,
        transport.as_ref(),
        &tracked_patterns,
        &logger,
        commit_message,
    )
    .await
    {
        Ok(result) => {
            if result.pulled > 0 {
                println!(
                    "  {} Pulled {} file(s).",
                    "OK".green().bold(),
                    result.pulled
                );
            }
            if result.pushed > 0 {
                println!(
                    "  {} Pushed {} file(s).",
                    "OK".green().bold(),
                    result.pushed
                );
            }
            if result.conflicts > 0 {
                let _ = logger.warn(&format!("{} conflict(s) detected", result.conflicts));
                println!(
                    "  {} {} conflict(s). Run `commitbook conflicts` to view.",
                    "WARN".yellow().bold(),
                    result.conflicts
                );
            }
            for err in &result.errors {
                let _ = logger.error(err);
                println!("  {} {}", "ERROR".red().bold(), err);
            }
            if result.pulled == 0 && result.pushed == 0 && result.conflicts == 0 {
                let _ = logger.info("Already up to date");
                println!("{}", "Already up to date.".dimmed());
            }
        }
        Err(e) => {
            let _ = logger.error(&format!("Sync failed: {e}"));
            println!("  {} Sync failed: {}", "ERROR".red().bold(), e);
            println!(
                "{}",
                "  Working tree preserved. Will retry on next sync.".dimmed()
            );
        }
    }

    let _ = logger.cleanup_old_logs();
    Ok(())
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

    // Commit + sync (same as manual, but with logging).
    if let Err(e) = run_sync(cb_dir, repo_root).await {
        let _ = logger.error(&format!("Scheduled sync failed: {e}"));
        log::error!("Scheduled sync failed: {e}");
    }

    let _ = logger.info("Scheduled sync cycle complete");
    log::info!("Scheduled sync cycle complete");

    // Cleanup lock.
    let _ = lock_file.unlock();
    let _ = std::fs::remove_file(&lock_path);

    Ok(())
}

/// Generate a commit message using AI or fallback.
async fn generate_commit_message(
    repo_root: &Path,
    summary: &commitbook_core::git::ChangesSummary,
) -> String {
    let chain = commitbook_core::ai::ProviderChain::new();
    // Default provider order — fallback is always last.
    let keys = vec![
        "gh-copilot".to_string(),
        "claude-cli".to_string(),
        "codex-cli".to_string(),
        "fallback".to_string(),
    ];
    let (msg, _provider) = chain.generate(summary, &keys, repo_root).await;
    msg
}

/// Build the transport used by the sync pipeline. Assumes the repo is
/// initialized and still has the remote named in `config.git.remote` — init
/// enforces exactly one remote, so any drift between config and reality here
/// is a setup error the user must fix.
fn create_transport(
    _cb_dir: &Path,
    repo_root: &Path,
    config: &LocalConfig,
) -> Result<Box<dyn commitbook_core::domain::transport::RemoteTransport>> {
    commitbook_core::git::remote::get_remote_url(repo_root, &config.git.remote).map_err(|_| {
        anyhow::anyhow!(
            "Configured remote `{}` not found in this git repo. \
             Re-run `commitbook init` after fixing your remotes.",
            config.git.remote
        )
    })?;

    Ok(Box::new(GitRemoteTransport::new(
        repo_root.to_path_buf(),
        config.git.remote.clone(),
        config.git.branch.clone(),
    )))
}

#[cfg(test)]
#[path = "sync_cmd_tests.rs"]
mod tests;
