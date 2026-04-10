use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_core::config::LocalConfig;
use commitbook_core::cron;

pub fn run(_cb_dir: &Path, repo_root: &Path, expression: &str) -> Result<()> {
    let mut config = LocalConfig::load(repo_root)?;

    // Resolve and validate.
    let schedule = match expression {
        "hourly" | "daily" | "every-30m" | "every-4h" => {
            cron::resolve_schedule(expression)
        }
        _ => {
            cron::validate_cron_expression(expression)?;
            expression.to_string()
        }
    };

    config.schedule = schedule.clone();
    config.save(repo_root)?;

    // Reinstall scheduler if it's currently running.
    if cron::is_loaded(repo_root) {
        let binary =
            std::env::current_exe().unwrap_or_else(|_| "commitbook".into());
        cron::install(repo_root, &schedule, &binary)?;
    }

    println!(
        "  {} Schedule updated: {}",
        "OK".green().bold(),
        cron::describe_schedule(&schedule).cyan()
    );

    Ok(())
}
