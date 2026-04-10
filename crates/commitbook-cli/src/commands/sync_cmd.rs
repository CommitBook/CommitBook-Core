use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_core::config::LocalConfig;
use commitbook_core::git::GitRepo;
use commitbook_core::state::auth::AuthConfig;
use commitbook_core::sync::scheduler::sync_repository;
use commitbook_core::transport::local_repo::LocalRepoTransport;

/// Run a manual sync: commit locally + pull -> merge -> push.
pub async fn run_sync(cb_dir: &Path, repo_root: &Path) -> Result<()> {
    let config = LocalConfig::load(repo_root)?;
    let repo = GitRepo::open(repo_root)?;

    // Step 1: Commit locally (always, even offline).
    let committed = commit_local(repo_root, &repo, &config).await?;
    if committed {
        println!("  {} Local changes committed.", "OK".green().bold());
    }

    // Step 2: Sync with remote (only if remote reachable).
    if !repo.has_remote() {
        println!(
            "{}",
            "  No remote configured. Commits are local only.".yellow()
        );
        return Ok(());
    }

    let transport = create_transport(cb_dir, repo_root, &config)?;
    let tracked_patterns: Vec<String> = config.files.include.clone();

    match sync_repository(
        cb_dir,
        repo_root,
        &config.git.branch,
        transport.as_ref(),
        &tracked_patterns,
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
                println!(
                    "  {} {} conflict(s). Run `commitbook conflicts` to view.",
                    "WARN".yellow().bold(),
                    result.conflicts
                );
            }
            for err in &result.errors {
                println!("  {} {}", "ERROR".red().bold(), err);
            }
            if result.pulled == 0
                && result.pushed == 0
                && result.conflicts == 0
                && !committed
            {
                println!("{}", "Already up to date.".dimmed());
            }
        }
        Err(e) => {
            println!(
                "  {} Sync failed: {}",
                "ERROR".red().bold(),
                e
            );
            println!(
                "{}",
                "  Local commit preserved. Will retry on next sync.".dimmed()
            );
        }
    }

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

    log::info!("Starting scheduled sync cycle");

    // Commit + sync (same as manual, but with logging).
    if let Err(e) = run_sync(cb_dir, repo_root).await {
        log::error!("Scheduled sync failed: {e}");
    }

    log::info!("Scheduled sync cycle complete");

    // Cleanup lock.
    let _ = lock_file.unlock();
    let _ = std::fs::remove_file(&lock_path);

    Ok(())
}

/// Commit local changes (git add + AI message + git commit).
async fn commit_local(
    repo_root: &Path,
    repo: &GitRepo,
    config: &LocalConfig,
) -> Result<bool> {
    let summary = repo.changes_summary()?;
    if summary.is_empty() {
        return Ok(false);
    }

    // Stage all changes.
    repo.stage_all()?;

    // Check for real diff.
    if !repo.has_real_staged_changes()? {
        return Ok(false);
    }

    // Generate commit message.
    let message = generate_commit_message(repo_root, &summary).await;

    // Create commit.
    repo.commit(&message)?;

    // Auto-push if configured and remote exists.
    if config.git.auto_push && repo.has_remote() {
        let remote = repo.default_remote_name().unwrap_or_else(|_| "origin".to_string());
        if let Err(e) = repo.push(&remote, &config.git.branch) {
            log::warn!("Auto-push failed: {e}");
        }
    }

    Ok(true)
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

/// Create the appropriate transport based on config and auth.
fn create_transport(
    cb_dir: &Path,
    repo_root: &Path,
    config: &LocalConfig,
) -> Result<Box<dyn commitbook_core::domain::transport::RemoteTransport>> {
    let auth = AuthConfig::load(cb_dir)?;

    if let Some(_token) = auth.token() {
        // TODO: Use PAT transport when available.
        anyhow::bail!(
            "PAT transport not yet implemented for sync. Use local repo transport."
        );
    }

    // Default: local repo transport (direct git operations).
    Ok(Box::new(LocalRepoTransport::new(
        repo_root.to_path_buf(),
        config.git.branch.clone(),
    )))
}
