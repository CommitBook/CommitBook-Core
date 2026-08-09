use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::cron;

pub fn run(_cb_dir: &Path, repo_root: &Path, expression: &str) -> Result<()> {
    let mut config = LocalConfig::load(repo_root)?;

    // Resolve and validate. Try presets first, then natural-time shorthands
    // (`5m`, `1h`, `1d`), and finally fall through to strict cron validation.
    let preset = cron::resolve_schedule(expression);
    let schedule = if preset != expression {
        preset
    } else if let Some(cron_expr) = cron::parse_human_interval(expression) {
        cron_expr
    } else {
        cron::validate_cron_expression(expression)?;
        expression.to_string()
    };
    cron::validate_platform_schedule(&schedule)?;

    config.schedule = schedule.clone();
    config.save(repo_root)?;

    // Reinstall scheduler if it's currently running.
    if cron::is_loaded(repo_root) {
        let binary = std::env::current_exe().unwrap_or_else(|_| "commitbook".into());
        cron::install(repo_root, &schedule, &binary)?;
    }

    println!(
        "  {} Schedule updated: {}",
        "OK".green().bold(),
        cron::describe_schedule(&schedule).cyan()
    );

    Ok(())
}
