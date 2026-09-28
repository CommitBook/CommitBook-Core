use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use commitbook_engine::cron::{self, SystemScheduler};
use commitbook_engine::settings::{self, SchedulerContext, SettingsUpdate, SettingsUpdateOutcome};

pub fn run(_cb_dir: &Path, repo_root: &Path, expression: &str) -> Result<()> {
    let context = SchedulerContext::new(&SystemScheduler, settings::current_binary());
    let outcome = run_with(repo_root, expression, &context)?;

    println!(
        "  {} Schedule updated: {}",
        "OK".green().bold(),
        cron::describe_schedule(&outcome.config.sync.schedule).cyan()
    );
    if outcome.scheduler_reinstalled {
        println!("  Scheduler reinstalled with the new schedule.");
    }

    Ok(())
}

/// Apply the schedule change through the shared locked settings operation.
/// Presets (`hourly`), shorthands (`5m`), and raw cron expressions are all
/// accepted; the scheduler is reinstalled only when it is currently running.
pub fn run_with(
    repo_root: &Path,
    expression: &str,
    context: &SchedulerContext<'_>,
) -> Result<SettingsUpdateOutcome> {
    let update = SettingsUpdate {
        schedule: Some(expression.to_string()),
        ..Default::default()
    };
    settings::update_settings(repo_root, &update, context)
}

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
