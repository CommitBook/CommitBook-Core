use anyhow::{bail, Result};
use colored::*;
use std::io::{self, Write};
use std::path::Path;

use commitbook_core::config::{GlobalConfig, LocalConfig};
use commitbook_core::cron;
use commitbook_core::git::GitRepo;

pub fn run(repo_path: &Path) -> Result<()> {
    println!("{}", "CommitBook Setup".bold().cyan());
    println!();

    // 1. Verify git repository
    print!("  Checking git repository... ");
    if !GitRepo::is_repo(repo_path) {
        println!("{}", "FAILED".red().bold());
        bail!("Directory is not a git repository: {}", repo_path.display());
    }
    println!("{}", "OK".green().bold());

    // 2. Check for git remote
    let repo = GitRepo::open(repo_path)?;
    print!("  Checking git remote... ");
    if repo.has_remote() {
        println!("{}", "OK".green().bold());
    } else {
        println!("{}", "WARN: no remote configured".yellow().bold());
        println!("    {}", "Auto-push will be disabled until a remote is added.".dimmed());
    }

    // 3. Check if already set up
    if LocalConfig::exists(repo_path) {
        println!();
        println!("  {} CommitBook is already set up in this repository.", "!".yellow().bold());
        print!("  Overwrite existing configuration? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("  Setup cancelled.");
            return Ok(());
        }
    }

    // 4. Ask for schedule
    println!();
    println!("  Select a commit schedule:");
    println!("    1) Every hour {}", "(default)".dimmed());
    println!("    2) Every 30 minutes");
    println!("    3) Every 4 hours");
    println!("    4) Daily at 9:00 AM");
    println!("    5) Custom cron expression");
    print!("  Choice [1]: ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    let choice = choice.trim();

    let schedule = match choice {
        "" | "1" => cron::resolve_schedule("hourly"),
        "2" => cron::resolve_schedule("every-30m"),
        "3" => cron::resolve_schedule("every-4h"),
        "4" => cron::resolve_schedule("daily"),
        "5" => {
            print!("  Enter cron expression: ");
            io::stdout().flush()?;
            let mut expr = String::new();
            io::stdin().read_line(&mut expr)?;
            let expr = expr.trim().to_string();
            cron::validate_cron_expression(&expr)?;
            expr
        }
        _ => cron::resolve_schedule("hourly"),
    };

    finish_setup(repo_path, &schedule)
}

fn finish_setup(repo_path: &Path, schedule: &str) -> Result<()> {
    // 5. Initialize .CommitBook directory
    print!("  Creating .CommitBook directory... ");
    let config = LocalConfig::init(repo_path, schedule)?;
    println!("{}", "OK".green().bold());

    // 6. Register in global config
    print!("  Registering repository... ");
    let repo_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut global = GlobalConfig::load()?;
    global.register_repo(&repo_str, schedule)?;
    println!("{}", "OK".green().bold());

    println!();
    println!("  {} CommitBook is set up!", "OK".green().bold());
    println!("  Schedule: {}", cron::describe_schedule(&config.schedule).cyan());
    println!();
    println!("  Run {} to start auto-commits.", "commitbook start".bold());
    println!("  Run {} to verify system health.", "commitbook doctor".bold());

    Ok(())
}
