use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

use commitbook_core::config::LocalConfig;
use commitbook_core::cron;
use commitbook_core::git::GitRepo;
use commitbook_core::state::auth::AuthConfig;

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

    // 9. Base directory.
    print!("  base/ directory... ");
    let base_dir = cb_dir.join("local").join("base");
    if base_dir.is_dir() {
        println!("{}", "OK".green().bold());
    } else {
        println!("{}", "MISSING".yellow().bold());
        println!(
            "    {}",
            "Will be created on first sync.".dimmed()
        );
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
