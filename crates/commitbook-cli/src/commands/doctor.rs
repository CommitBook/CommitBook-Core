use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

use commitbook_core::config::LocalConfig;
use commitbook_core::cron;
use commitbook_core::git::GitRepo;
use commitbook_core::state::auth::AuthConfig;
use commitbook_core::state::sync_state::SyncState;

pub fn run(cb_dir: &Path, repo_root: &Path, _json: bool) -> Result<()> {
    println!("{}", "CommitBook Doctor".bold().cyan());
    println!();

    let mut all_ok = true;

    // 1. Git.
    print!("  Git installed... ");
    match Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => {
            println!("{}", "OK".green().bold());
        }
        _ => {
            println!("{}", "FAILED".red().bold());
            all_ok = false;
        }
    }

    // 2. Git repo.
    print!("  Git repository... ");
    if GitRepo::is_repo(repo_root) {
        println!("{}", "OK".green().bold());
    } else {
        println!("{}", "FAILED".red().bold());
        println!("    {}", "Not a git repository.".dimmed());
        all_ok = false;
    }

    // 3. Remote.
    print!("  Git remote... ");
    if let Ok(repo) = GitRepo::open(repo_root) {
        if repo.has_remote() {
            println!("{}", "OK".green().bold());
        } else {
            println!("{}", "WARN".yellow().bold());
            println!(
                "    {}",
                "No remote configured. Sync will be local only.".dimmed()
            );
        }
    } else {
        println!("{}", "SKIP".dimmed());
    }

    // 4. .CommitBook/ structure.
    print!("  .CommitBook/ directory... ");
    if cb_dir.is_dir() {
        println!("{}", "OK".green().bold());
    } else {
        println!("{}", "FAILED".red().bold());
        all_ok = false;
    }

    // 5. config.toml.
    print!("  config.toml... ");
    if LocalConfig::exists(repo_root) {
        println!("{}", "OK".green().bold());
    } else {
        println!("{}", "MISSING".yellow().bold());
    }

    // 6. Auth.
    print!("  auth.toml... ");
    match AuthConfig::load(cb_dir) {
        Ok(auth) if auth.has_token() => {
            println!("{}", "OK".green().bold());
        }
        Ok(_) => {
            println!("{}", "not configured".dimmed());
        }
        Err(_) => {
            println!("{}", "error reading".red());
            all_ok = false;
        }
    }

    // 7. Scheduler.
    print!("  Scheduler... ");
    if cron::is_loaded(repo_root) {
        println!("{}", "running".green().bold());
    } else {
        println!("{}", "stopped".dimmed());
    }

    // 8. AI providers.
    print!("  AI providers... ");
    let chain = commitbook_core::ai::ProviderChain::new();
    let keys = vec![
        "gh-copilot".to_string(),
        "claude-cli".to_string(),
        "codex-cli".to_string(),
    ];
    let availability = chain.check_availability(&keys);
    let available: Vec<_> = availability
        .iter()
        .filter(|(_, _, avail)| *avail)
        .map(|(_, name, _)| name.as_str())
        .collect();
    if available.is_empty() {
        println!("{}", "none found".yellow());
        println!(
            "    {}",
            "Commit messages will use fallback text.".dimmed()
        );
    } else {
        println!(
            "{}",
            available.join(", ").green()
        );
    }

    // 9. Sync checkpoint.
    print!("  Sync checkpoint... ");
    match SyncState::load(cb_dir) {
        Ok(state) if state.remote_head.is_some() => {
            println!("{}", "OK".green().bold());
        }
        _ => {
            println!("{}", "not yet established".dimmed());
            println!(
                "    {}",
                "Will be set after the first successful sync.".dimmed()
            );
        }
    }

    // 10. Local vs origin divergence.
    if let Ok(repo) = GitRepo::open(repo_root) {
        if repo.has_remote() {
            let config = LocalConfig::load(repo_root).ok();
            let branch = config
                .as_ref()
                .map(|c| c.git.branch.clone())
                .unwrap_or_else(|| "main".to_string());
            let remote_ref = format!("origin/{branch}");

            print!("  Local vs {}... ", remote_ref);
            match repo.ahead_behind("HEAD", &remote_ref) {
                Ok((0, 0)) => println!("{}", "in sync".green().bold()),
                Ok((ahead, 0)) => {
                    println!(
                        "{}",
                        format!("{} commit(s) ahead", ahead).yellow().bold()
                    );
                    println!(
                        "    {}",
                        "Next sync will try to push.".dimmed()
                    );
                }
                Ok((0, behind)) => {
                    println!(
                        "{}",
                        format!("{} commit(s) behind", behind).yellow().bold()
                    );
                    println!(
                        "    Run: {}",
                        format!("git pull --ff-only").dimmed()
                    );
                }
                Ok((ahead, behind)) => {
                    println!("{}", "DIVERGED".red().bold());
                    println!(
                        "    {} ahead, {} behind — local and origin have different histories.",
                        ahead, behind
                    );
                    println!(
                        "    If CommitBook has been pushing your content, local-only commits are redundant. Recovery:"
                    );
                    println!(
                        "    {}",
                        "git fetch origin".dimmed()
                    );
                    println!(
                        "    {}",
                        format!("git diff {} -- '*.md' '*.markdown'", remote_ref).dimmed()
                    );
                    println!(
                        "    If the diff is empty/expected:  {}",
                        format!("git reset --hard {}", remote_ref).dimmed()
                    );
                    println!(
                        "    Otherwise, merge by hand:       {}",
                        format!("git merge {}", remote_ref).dimmed()
                    );
                    all_ok = false;
                }
                Err(_) => {
                    println!("{}", "SKIP".dimmed());
                    println!(
                        "    {}",
                        "Could not determine divergence (fetch may have failed).".dimmed()
                    );
                }
            }
        }
    }

    println!();
    if all_ok {
        println!(
            "  {}",
            "All checks passed.".green().bold()
        );
    } else {
        println!(
            "  {}",
            "Some checks failed. See above.".red().bold()
        );
    }

    Ok(())
}
