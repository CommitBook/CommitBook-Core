use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::cron;
use commitbook_engine::git::GitRepo;
use commitbook_engine::state::auth::AuthConfig;
use commitbook_engine::state::sync_state::SyncState;

pub fn run(cb_dir: &Path, repo_root: &Path, _json: bool, fix: bool) -> Result<()> {
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

    // 3. Remote, CommitBook requires exactly one remote. Also verify the
    //    name in config matches what's actually configured.
    print!("  Git remote... ");
    match commitbook_engine::git::remote::list_remote_names(repo_root) {
        Ok(names) if names.len() == 1 => {
            let actual = &names[0];
            let configured = LocalConfig::load(repo_root).ok().map(|c| c.git.remote);
            if let Some(cfg_remote) = configured {
                if cfg_remote == *actual {
                    println!("{} ({})", "OK".green().bold(), actual);
                } else {
                    println!("{}", "MISMATCH".red().bold());
                    println!(
                        "    Config says `{}`, git has `{}`. Re-run `commitbook init`.",
                        cfg_remote, actual
                    );
                    all_ok = false;
                }
            } else {
                println!("{} ({})", "OK".green().bold(), actual);
            }
        }
        Ok(names) if names.is_empty() => {
            println!("{}", "FAILED".red().bold());
            println!(
                "    {}",
                "No remote configured. CommitBook requires exactly 1 remote.".dimmed()
            );
            all_ok = false;
        }
        Ok(names) => {
            println!("{}", "FAILED".red().bold());
            println!(
                "    Found {} remotes ({}). CommitBook requires exactly 1.",
                names.len(),
                names.join(", ")
            );
            all_ok = false;
        }
        Err(_) => {
            println!("{}", "SKIP".dimmed());
        }
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

    // 6. Optional token-backed auth. Normal desktop sync uses the user's
    // system Git credentials (SSH agent, keychain, .git-credentials, etc.).
    print!("  Token auth (optional)... ");
    match AuthConfig::load(cb_dir) {
        Ok(auth) if auth.has_token() => {
            println!("{}", "OK".green().bold());
        }
        Ok(_) => {
            println!("{}", "not configured".dimmed());
            println!(
                "    {}",
                "Normal for desktop git sync; `commitbook token set` is only needed for token-backed transports.".dimmed()
            );
        }
        Err(_) => {
            println!("{}", "error reading auth.toml".red());
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

    // 8. AI providers. Skipped entirely when `[commit] ai_messages = false`,
    //    since sync uses only the deterministic timestamp fallback then.
    print!("  AI providers... ");
    let ai_messages = LocalConfig::load(repo_root)
        .map(|c| c.commit.ai_messages)
        .unwrap_or(false);
    if !ai_messages {
        println!("{}", "disabled".dimmed());
        println!(
            "    {}",
            "[commit] ai_messages = false; commit messages use timestamp text.".dimmed()
        );
    } else {
        let chain = commitbook_engine::ai::ProviderChain::new();
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
            println!("    {}", "Commit messages will use fallback text.".dimmed());
        } else {
            println!("{}", available.join(", ").green());
        }
    }

    // 9. Sync checkpoint.
    print!("  Sync checkpoint... ");
    match SyncState::load(cb_dir) {
        Ok(state) if state.last_sync_at.is_some() => {
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

    // 10. Local vs <remote> divergence.
    if let Ok(repo) = GitRepo::open(repo_root) {
        if repo.has_remote() {
            let config = LocalConfig::load(repo_root).ok();
            let branch = config
                .as_ref()
                .map(|c| c.git.branch.clone())
                .unwrap_or_else(|| "main".to_string());
            let remote_name = config
                .as_ref()
                .map(|c| c.git.remote.clone())
                .unwrap_or_else(|| "origin".to_string());
            let remote_ref = format!("{remote_name}/{branch}");

            print!("  Local vs {}... ", remote_ref);
            match repo.ahead_behind("HEAD", &remote_ref) {
                Ok((0, 0)) => println!("{}", "in sync".green().bold()),
                Ok((ahead, 0)) => {
                    println!("{}", format!("{} commit(s) ahead", ahead).yellow().bold());
                    println!("    {}", "Next sync will try to push.".dimmed());
                }
                Ok((0, behind)) => {
                    println!("{}", format!("{} commit(s) behind", behind).yellow().bold());
                    println!("    Run: {}", "git pull --ff-only".dimmed());
                }
                Ok((ahead, behind)) => {
                    println!("{}", "DIVERGED".red().bold());
                    println!(
                        "    {} ahead, {} behind: local and {} have different histories.",
                        ahead, behind, remote_name
                    );
                    println!(
                        "    If CommitBook has been pushing your content, local-only commits are redundant. Recovery:"
                    );
                    println!("    {}", format!("git fetch {}", remote_name).dimmed());
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

    if fix {
        println!();
        println!("  {}", "Auto-repair:".bold().cyan());
        let repaired = run_fixes(cb_dir, repo_root);
        if repaired > 0 {
            // Do not force `all_ok`: the exit status must keep reflecting the
            // checks so `--fix` cannot mask a still-failing condition.
            println!(
                "  {}",
                "Repairs applied. Re-run `commitbook doctor` to verify.".dimmed()
            );
        }
    }

    println!();
    if all_ok {
        println!("  {}", "All checks passed.".green().bold());
        Ok(())
    } else {
        println!("  {}", "Some checks failed. See above.".red().bold());
        if !fix {
            println!(
                "  {}",
                "Try `commitbook doctor --fix` to auto-repair common issues.".dimmed()
            );
        }
        Err(anyhow::anyhow!("doctor reported one or more failures"))
    }
}

/// Run auto-repair for known-fixable cases. Returns the number of fixes
/// successfully applied. Each fix prints its own status line.
fn run_fixes(_cb_dir: &Path, repo_root: &Path) -> u32 {
    let mut fixed = 0u32;

    if fix_logs_dir(repo_root) {
        fixed += 1;
    }
    if fix_plist_binary_path(repo_root) {
        fixed += 1;
    }

    if fixed == 0 {
        println!("    {}", "Nothing to repair.".dimmed());
    }

    fixed
}

/// Recreate `.CommitBook/local/logs/` if missing. Cheap and idempotent.
fn fix_logs_dir(repo_root: &Path) -> bool {
    let logs = LocalConfig::logs_dir(repo_root);
    if logs.is_dir() {
        return false;
    }
    print!("    Creating logs/... ");
    match std::fs::create_dir_all(&logs) {
        Ok(()) => {
            println!("{}", "OK".green().bold());
            true
        }
        Err(e) => {
            println!("{} {}", "FAILED".red().bold(), e);
            false
        }
    }
}

/// If a launchd plist exists for this repo and points at a stale binary path
/// (e.g., a workspace that no longer exists, or a target/debug from a
/// different checkout), reinstall it pointing at the binary actually running
/// `commitbook doctor` right now.
#[cfg(not(target_os = "macos"))]
fn fix_plist_binary_path(_repo_root: &Path) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn fix_plist_binary_path(repo_root: &Path) -> bool {
    let Ok(current_exe) = std::env::current_exe() else {
        return false;
    };

    let legacy_path = commitbook_engine::cron::macos::legacy_plist_path(repo_root);
    let legacy_exists = legacy_path.exists();
    let legacy_loaded = commitbook_engine::cron::macos::is_legacy_loaded(repo_root);
    let binary_is_current = commitbook_engine::cron::macos::existing_plist_path(repo_root)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|contents| contents.contains(&current_exe.to_string_lossy().to_string()));
    if !legacy_exists && !legacy_loaded && binary_is_current {
        return false;
    }

    // A dormant legacy plist is stale state: `commitbook stop` removes both
    // labels, and doctor must not unexpectedly start a stopped scheduler.
    // Remove that file directly when neither label is loaded.
    if !cron::is_loaded(repo_root) {
        if legacy_exists {
            print!("    Removing legacy scheduler plist... ");
            return match std::fs::remove_file(&legacy_path) {
                Ok(()) => {
                    println!("{}", "OK".green().bold());
                    true
                }
                Err(e) => {
                    println!("{} {}", "FAILED".red().bold(), e);
                    false
                }
            };
        }
        return false;
    }

    print!("    Reinstalling scheduler with current label and binary path... ");
    let config = match LocalConfig::load(repo_root) {
        Ok(c) => c,
        Err(e) => {
            println!("{} {}", "FAILED".red().bold(), e);
            return false;
        }
    };
    match cron::install(repo_root, &config.schedule, &current_exe) {
        Ok(_) => {
            println!("{}", "OK".green().bold());
            true
        }
        Err(e) => {
            println!("{} {}", "FAILED".red().bold(), e);
            false
        }
    }
}
