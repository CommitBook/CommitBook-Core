use anyhow::Result;
use colored::*;
use std::path::Path;

use commitbook_core::config::LocalConfig;
use commitbook_core::cron;
use commitbook_core::git::remote;

pub fn run(repo_path: &Path) -> Result<()> {
    if !LocalConfig::exists(repo_path) {
        println!("{}", "CommitBook is not configured for this repository.".yellow());
        println!("Run {} to initialize.", "commitbook setup".bold());
        return Ok(());
    }

    let config = LocalConfig::load(repo_path)?;
    let running = cron::is_loaded(repo_path);

    println!("{}", "CommitBook Status".bold().cyan());
    println!();

    // State
    if running {
        println!("  State:       {}", "Running".green().bold());
    } else {
        println!("  State:       {}", "Stopped".yellow().bold());
    }

    // Schedule
    println!("  Schedule:    {}", cron::describe_schedule(&config.schedule));

    // Last commit
    if let Some(ref last) = config.last_commit {
        if let Ok(last_time) = chrono::DateTime::parse_from_str(last, "%Y-%m-%dT%H:%M:%SZ") {
            let local_time = last_time.with_timezone(&chrono::Local);
            let ago = chrono::Local::now().signed_duration_since(local_time);
            let ago_str = commitbook_core::utils::datetime::format_relative(ago.num_seconds());
            println!(
                "  Last commit: {} ({})",
                local_time.format("%Y-%m-%d %H:%M:%S"),
                ago_str
            );
        } else {
            println!("  Last commit: {}", last);
        }
    } else {
        println!("  Last commit: {}", "Never".dimmed());
    }

    // Next commit estimate
    if running {
        if let Some(ref last) = config.last_commit {
            if let Ok(last_time) = chrono::DateTime::parse_from_str(last, "%Y-%m-%dT%H:%M:%SZ") {
                let interval = cron::cron_to_interval_seconds(&config.schedule);
                let next_time = last_time + chrono::Duration::seconds(interval as i64);
                let now = chrono::Utc::now().fixed_offset();
                if next_time > now {
                    let until = (next_time - now).num_seconds();
                    let until_str = format_until(until);
                    println!(
                        "  Next commit: {} (in {})",
                        next_time.with_timezone(&chrono::Local).format("%H:%M:%S"),
                        until_str
                    );
                } else {
                    println!("  Next commit: {}", "Imminent".green());
                }
            }
        } else {
            println!("  Next commit: {}", "Pending (first run)".dimmed());
        }
    } else {
        println!("  Next commit: {}", "N/A (stopped)".dimmed());
    }

    println!();

    // Repository info
    println!("  Repo:        {}", repo_path.display());
    println!("  Branch:      {}", config.git.branch);
    println!("  Auto-push:   {}", config.git.auto_push);

    if let Ok(url) = remote::get_remote_url(repo_path, "origin") {
        println!("  Remote:      {}", url);
    }

    Ok(())
}

fn format_until(seconds: i64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        if m > 0 {
            format!("{}h {}m", h, m)
        } else {
            format!("{}h", h)
        }
    }
}
