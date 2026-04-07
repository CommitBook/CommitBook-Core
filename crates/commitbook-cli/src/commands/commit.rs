use anyhow::Result;
use colored::*;
use std::path::Path;

use commitbook_core::ai::ProviderChain;
use commitbook_core::config::{GlobalConfig, LocalConfig};
use commitbook_core::git::GitRepo;
use commitbook_core::utils::datetime;

/// Run a manual commit cycle.
pub async fn run(repo_path: &Path, dry_run: bool, message: Option<&str>) -> Result<()> {
    if dry_run {
        println!("{}", "CommitBook Commit (dry run)".bold().cyan());
    } else {
        println!("{}", "CommitBook Commit".bold().cyan());
    }
    println!();

    let repo = GitRepo::open(repo_path)?;

    // Check for changes
    let summary = repo.changes_summary()?;
    if summary.is_empty() {
        println!("  {} No changes detected.", "!".yellow().bold());
        return Ok(());
    }

    println!("  Changes: {}", summary.to_detail_text());

    if dry_run {
        // Just show what would happen
        let commit_msg = if let Some(msg) = message {
            msg.to_string()
        } else {
            let global = GlobalConfig::load().unwrap_or_default();
            let chain = ProviderChain::new();
            let (msg, provider) = chain
                .generate(&summary, &global.ai.providers, repo_path)
                .await;
            println!("  Provider: {}", provider.cyan());
            msg
        };
        println!("  Message:  {}", commit_msg.green());
        println!();
        println!("  {} Dry run complete. No changes were committed.", "OK".green().bold());
        return Ok(());
    }

    // Stage all
    repo.stage_all()?;

    // Empty diff guard
    if !repo.has_real_staged_changes()? {
        println!("  {} No real content changes (mtime-only).", "!".yellow().bold());
        return Ok(());
    }

    // Generate or use provided message
    let (commit_msg, provider_name) = if let Some(msg) = message {
        (msg.to_string(), "manual".to_string())
    } else {
        let global = GlobalConfig::load().unwrap_or_default();
        let chain = ProviderChain::new();
        chain
            .generate(&summary, &global.ai.providers, repo_path)
            .await
    };

    println!("  Provider: {}", provider_name.cyan());
    println!("  Message:  {}", commit_msg.green());

    // Commit
    let short_hash = repo.commit(&commit_msg)?;
    println!();
    println!("  {} Committed: {}", "OK".green().bold(), short_hash);

    // Push if configured
    if LocalConfig::exists(repo_path) {
        let config = LocalConfig::load(repo_path)?;
        if config.git.auto_push && repo.has_remote() {
            let remote = repo.default_remote_name().unwrap_or_else(|_| "origin".to_string());
            let branch = config.git.branch.clone();

            print!("  Pushing to {}/{}... ", remote, branch);
            match repo.push(&remote, &branch) {
                Ok(()) => println!("{}", "OK".green().bold()),
                Err(e) => println!("{}: {}", "FAILED".red().bold(), e),
            }
        }
    }

    // Update last_commit
    if LocalConfig::exists(repo_path) {
        let mut config = LocalConfig::load(repo_path)?;
        config.last_commit = Some(datetime::now_iso());
        config.save(repo_path)?;
    }

    Ok(())
}
