//! Locked settings updates shared by the CLI, web dashboard, and TUI.
//!
//! Every mutation of `.CommitBook/config.toml` that a user interface performs
//! goes through here so that concurrent writers are rejected by the
//! repository lock, the schedule is validated once, and an installed
//! scheduler is reinstalled (or rolled back) whenever the schedule changes.

use anyhow::{bail, Context, Result};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::{Agent, CommitAgent, CommitMode, ConflictMode, LocalConfig, LogKeep};
use crate::cron::{self, SchedulerAdapter};
use crate::git::GitRepo;
use crate::state::RepoLock;

/// Longest accepted `[commitbook] name`, matching device names.
const MAX_NAME_LEN: usize = 64;

/// Fields a user interface may change. `None` leaves the field untouched.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SettingsUpdate {
    pub name: Option<String>,
    pub schedule: Option<String>,
    pub branch: Option<String>,
    pub commit_mode: Option<CommitMode>,
    pub commit_agent: Option<CommitAgent>,
    pub conflict_mode: Option<ConflictMode>,
    pub conflict_agent: Option<Agent>,
    pub log_keep: Option<LogKeep>,
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
/// The last resort is the current executable, which `cron::install` then
/// refuses unless it is the CLI.
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

/// Turn user input (`hourly`, `5m`, or a cron expression) into the value
/// stored in `[sync] schedule`, validated for this platform's scheduler.
pub fn normalize_schedule(input: &str) -> Result<String> {
    cron::normalize_schedule(input)
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

fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("Name must not be empty");
    }
    if name.chars().any(char::is_control) {
        bail!("Name must not contain control characters");
    }
    if name.chars().count() > MAX_NAME_LEN {
        bail!("Name must be at most {MAX_NAME_LEN} characters");
    }
    Ok(name.to_string())
}

/// Sync refuses to run on any branch but the configured one, so a new
/// branch is accepted only once it is checked out.
fn ensure_checked_out(repo_root: &Path, branch: &str) -> Result<()> {
    let checked_out = GitRepo::open(repo_root)?.current_branch()?;
    if checked_out != branch {
        bail!("Check out `{branch}` first: CommitBook syncs the checked-out branch, which is `{checked_out}`");
    }
    Ok(())
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

    if let Some(name) = &update.name {
        let name = validate_name(name)?;
        changed |= new.commitbook.name != name;
        new.commitbook.name = name;
    }
    if let Some(schedule) = &update.schedule {
        let schedule = normalize_schedule(schedule)?;
        changed |= new.sync.schedule != schedule;
        new.sync.schedule = schedule;
    }
    if let Some(branch) = &update.branch {
        let branch = validate_branch(branch)?;
        if new.git.branch != branch {
            ensure_checked_out(repo_root, &branch)?;
            changed = true;
        }
        new.git.branch = branch;
    }
    if let Some(mode) = update.commit_mode {
        changed |= new.commit.mode != mode;
        new.commit.mode = mode;
    }
    if let Some(agent) = update.commit_agent {
        changed |= new.commit.agent != agent;
        new.commit.agent = agent;
    }
    if let Some(mode) = update.conflict_mode {
        changed |= new.conflicts.mode != mode;
        new.conflicts.mode = mode;
    }
    if let Some(agent) = update.conflict_agent {
        changed |= new.conflicts.agent != agent;
        new.conflicts.agent = agent;
    }
    if let Some(keep) = update.log_keep {
        changed |= new.logs.keep != keep;
        new.logs.keep = keep;
    }
    let schedule_changed = new.sync.schedule != old.sync.schedule;
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
        .install(repo_root, &new.sync.schedule, &scheduler.binary)
    {
        Ok(()) => Ok(SettingsUpdateOutcome {
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
                        .install(repo_root, &old.sync.schedule, &scheduler.binary)
                        .context("Failed to reinstall the previous scheduler job")
                })
                .err();
            Err(SettingsUpdateError { original, rollback }.into())
        }
    }
}

/// Install the scheduler job for the configured schedule under the lock.
pub fn start_scheduler(repo_root: &Path, scheduler: &SchedulerContext<'_>) -> Result<()> {
    let lock = RepoLock::acquire(repo_root)?;
    start_scheduler_with_lock(repo_root, scheduler, &lock)
}

pub fn start_scheduler_with_lock(
    repo_root: &Path,
    scheduler: &SchedulerContext<'_>,
    lock: &RepoLock,
) -> Result<()> {
    lock.ensure_matches(repo_root)?;
    let config = LocalConfig::load(repo_root)?;
    scheduler
        .adapter
        .install(repo_root, &config.sync.schedule, &scheduler.binary)
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
    scheduler.adapter.uninstall(repo_root)
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
