//! Locked settings updates shared by the CLI, web dashboard, and TUI.
//!
//! Every mutation of `.CommitBook/config.toml` that a user interface performs
//! goes through here so that concurrent writers are rejected by the
//! repository lock, the schedule is validated once, and an installed
//! scheduler is reinstalled (or rolled back) whenever the schedule changes.

use anyhow::{bail, Context, Result};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::LocalConfig;
use crate::cron::{self, SchedulerAdapter};
use crate::state::RepoLock;

/// Fields a user interface may change. `None` leaves the field untouched.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SettingsUpdate {
    pub schedule: Option<String>,
    pub branch: Option<String>,
    pub auto_push: Option<bool>,
    pub ai_messages: Option<bool>,
    pub enabled: Option<bool>,
    pub review_ai_resolutions: Option<bool>,
    pub resolver: Option<String>,
    pub auto_merge_appends: Option<bool>,
}

impl SettingsUpdate {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The scheduler to keep in sync with the configuration and the binary its
/// job should invoke.
pub struct SchedulerContext<'a> {
    pub adapter: &'a dyn SchedulerAdapter,
    pub binary: PathBuf,
}

impl<'a> SchedulerContext<'a> {
    pub fn new(adapter: &'a dyn SchedulerAdapter, binary: PathBuf) -> Self {
        Self { adapter, binary }
    }
}

/// Resolve the `commitbook` binary a scheduler job should run: the current
/// executable when it is the CLI, otherwise a `commitbook` sibling of the
/// current executable, otherwise whatever `commitbook` resolves to on PATH.
pub fn current_binary() -> PathBuf {
    let current = std::env::current_exe().ok();
    if let Some(current) = &current {
        let is_cli = current
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == "commitbook");
        if is_cli {
            return current.clone();
        }
        if let Some(sibling) = current.parent().map(|dir| dir.join("commitbook")) {
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    if let Ok(found) = which::which("commitbook") {
        return found;
    }
    current.unwrap_or_else(|| PathBuf::from("commitbook"))
}

/// Result of a successful settings update.
#[derive(Debug)]
pub struct SettingsUpdateOutcome {
    pub config: LocalConfig,
    pub schedule_changed: bool,
    pub scheduler_reinstalled: bool,
}

/// Returned when the new configuration was saved but the active scheduler
/// could not be updated to match it. The previous configuration is restored
/// and the previous job reinstalled; `rollback` records whether that also
/// failed.
#[derive(Debug)]
pub struct SettingsUpdateError {
    pub original: anyhow::Error,
    pub rollback: Option<anyhow::Error>,
}

impl fmt::Display for SettingsUpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Failed to apply the new schedule to the active scheduler: {:#}",
            self.original
        )?;
        match &self.rollback {
            Some(rollback) => write!(
                f,
                "; restoring the previous configuration and job also failed: {rollback:#}"
            ),
            None => write!(f, "; the previous configuration and job were restored"),
        }
    }
}

impl std::error::Error for SettingsUpdateError {}

/// Turn user input (`hourly`, `5m`, or a cron expression) into a validated
/// cron expression this platform's scheduler can represent.
pub fn normalize_schedule(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("Schedule must not be empty");
    }
    let preset = cron::resolve_schedule(trimmed);
    let schedule = if preset != trimmed {
        preset
    } else if let Some(expression) = cron::parse_human_interval(trimmed) {
        expression
    } else {
        cron::validate_cron_expression(trimmed)?;
        trimmed.to_string()
    };
    cron::validate_platform_schedule(&schedule)?;
    Ok(schedule)
}

fn validate_branch(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("Branch name must not be empty");
    }
    if trimmed.chars().any(char::is_whitespace) {
        bail!("Branch name must not contain whitespace: `{trimmed}`");
    }
    let valid = git2::Branch::name_is_valid(trimmed)
        .with_context(|| format!("Failed to validate branch name `{trimmed}`"))?;
    if !valid {
        bail!("Invalid branch name: `{trimmed}`");
    }
    Ok(trimmed.to_string())
}

/// Apply `update` under the repository lock.
pub fn update_settings(
    repo_root: &Path,
    update: &SettingsUpdate,
    scheduler: &SchedulerContext<'_>,
) -> Result<SettingsUpdateOutcome> {
    let lock = RepoLock::acquire(repo_root)?;
    update_settings_with_lock(repo_root, update, scheduler, &lock)
}

/// Apply `update` while the caller already holds the repository lock.
pub fn update_settings_with_lock(
    repo_root: &Path,
    update: &SettingsUpdate,
    scheduler: &SchedulerContext<'_>,
    lock: &RepoLock,
) -> Result<SettingsUpdateOutcome> {
    lock.ensure_matches(repo_root)?;
    let old = LocalConfig::load(repo_root)?;
    let mut new = old.clone();
    let mut changed = false;

    if let Some(schedule) = &update.schedule {
        let schedule = normalize_schedule(schedule)?;
        if new.schedule != schedule {
            new.schedule = schedule;
            changed = true;
        }
    }
    if let Some(branch) = &update.branch {
        let branch = validate_branch(branch)?;
        if new.git.branch != branch {
            new.git.branch = branch;
            changed = true;
        }
    }
    if let Some(auto_push) = update.auto_push {
        if new.git.auto_push != auto_push {
            new.git.auto_push = auto_push;
            changed = true;
        }
    }
    if let Some(ai_messages) = update.ai_messages {
        if new.commit.ai_messages != ai_messages {
            new.commit.ai_messages = ai_messages;
            changed = true;
        }
    }
    if let Some(enabled) = update.enabled {
        if new.enabled != enabled {
            new.enabled = enabled;
            changed = true;
        }
    }

    if let Some(auto_merge) = update.auto_merge_appends {
        changed |= new.conflict.auto_merge_appends != auto_merge;
        new.conflict.auto_merge_appends = auto_merge;
    }
    if let Some(review) = update.review_ai_resolutions {
        changed |= new.conflict.review_ai_resolutions != review;
        new.conflict.review_ai_resolutions = review;
    }
    if let Some(resolver) = &update.resolver {
        anyhow::ensure!(
            ["manual", "claude", "codex", "copilot", "gemini", "cursor"]
                .contains(&resolver.as_str()),
            "Unknown conflict resolver"
        );
        changed |= new.conflict.resolver != *resolver;
        new.conflict.resolver = resolver.clone();
    }
    let schedule_changed = new.schedule != old.schedule;
    if !changed {
        return Ok(SettingsUpdateOutcome {
            config: new,
            schedule_changed: false,
            scheduler_reinstalled: false,
        });
    }

    new.save(repo_root)?;

    if !schedule_changed || !scheduler.adapter.is_loaded(repo_root) {
        return Ok(SettingsUpdateOutcome {
            config: new,
            schedule_changed,
            scheduler_reinstalled: false,
        });
    }

    match scheduler
        .adapter
        .install(repo_root, &new.schedule, &scheduler.binary)
    {
        Ok(_) => Ok(SettingsUpdateOutcome {
            config: new,
            schedule_changed,
            scheduler_reinstalled: true,
        }),
        Err(original) => {
            let rollback = old
                .save(repo_root)
                .context("Failed to restore the previous configuration")
                .and_then(|()| {
                    scheduler
                        .adapter
                        .install(repo_root, &old.schedule, &scheduler.binary)
                        .map(|_| ())
                        .context("Failed to reinstall the previous scheduler job")
                })
                .err();
            Err(SettingsUpdateError { original, rollback }.into())
        }
    }
}

/// Install the scheduler job for the configured schedule under the lock.
pub fn start_scheduler(repo_root: &Path, scheduler: &SchedulerContext<'_>) -> Result<String> {
    let lock = RepoLock::acquire(repo_root)?;
    start_scheduler_with_lock(repo_root, scheduler, &lock)
}

pub fn start_scheduler_with_lock(
    repo_root: &Path,
    scheduler: &SchedulerContext<'_>,
    lock: &RepoLock,
) -> Result<String> {
    lock.ensure_matches(repo_root)?;
    let config = LocalConfig::load(repo_root)?;
    scheduler
        .adapter
        .install(repo_root, &config.schedule, &scheduler.binary)
}

/// Remove the scheduler job under the lock.
pub fn stop_scheduler(repo_root: &Path, scheduler: &SchedulerContext<'_>) -> Result<()> {
    let lock = RepoLock::acquire(repo_root)?;
    stop_scheduler_with_lock(repo_root, scheduler, &lock)
}

pub fn stop_scheduler_with_lock(
    repo_root: &Path,
    scheduler: &SchedulerContext<'_>,
    lock: &RepoLock,
) -> Result<()> {
    lock.ensure_matches(repo_root)?;
    scheduler.adapter.uninstall(repo_root, None)
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
