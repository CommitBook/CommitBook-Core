//! In-memory scheduler used by tests across the workspace. It records every
//! call and can be told to fail the next install or uninstall so rollback
//! paths can be exercised without launchd or crontab.

use anyhow::{anyhow, Result};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::SchedulerAdapter;

/// One recorded scheduler interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeCall {
    Install { schedule: String, binary: PathBuf },
    Uninstall,
    IsLoaded,
}

#[derive(Debug, Default)]
struct FakeState {
    installed: Option<(String, PathBuf)>,
    install_failures: VecDeque<String>,
    uninstall_failures: VecDeque<String>,
    calls: Vec<FakeCall>,
}

/// Test double for [`SchedulerAdapter`].
#[derive(Debug, Default)]
#[doc(hidden)]
pub struct FakeScheduler {
    state: Mutex<FakeState>,
}

impl FakeScheduler {
    /// A scheduler with no job installed.
    pub fn stopped() -> Self {
        Self::default()
    }

    /// A scheduler that already has a job installed with `schedule`.
    pub fn running(schedule: &str) -> Self {
        let fake = Self::default();
        fake.lock().installed = Some((schedule.to_string(), PathBuf::from("commitbook")));
        fake
    }

    /// Make the next `install` call fail with `message`. Calls stack.
    pub fn fail_next_install(&self, message: &str) {
        self.lock().install_failures.push_back(message.to_string());
    }

    /// Make the next `uninstall` call fail with `message`. Calls stack.
    pub fn fail_next_uninstall(&self, message: &str) {
        self.lock()
            .uninstall_failures
            .push_back(message.to_string());
    }

    /// The schedule of the currently installed job, if any.
    pub fn installed_schedule(&self) -> Option<String> {
        self.lock()
            .installed
            .as_ref()
            .map(|(schedule, _)| schedule.clone())
    }

    /// Every call made so far, in order.
    pub fn calls(&self) -> Vec<FakeCall> {
        self.lock().calls.clone()
    }

    /// Number of `install` calls made so far, successful or not.
    pub fn install_count(&self) -> usize {
        self.lock()
            .calls
            .iter()
            .filter(|call| matches!(call, FakeCall::Install { .. }))
            .count()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SchedulerAdapter for FakeScheduler {
    fn install(&self, _repo_root: &Path, schedule: &str, binary: &Path) -> Result<()> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Install {
            schedule: schedule.to_string(),
            binary: binary.to_path_buf(),
        });
        if let Some(message) = state.install_failures.pop_front() {
            return Err(anyhow!("{message}"));
        }
        state.installed = Some((schedule.to_string(), binary.to_path_buf()));
        Ok(())
    }

    fn uninstall(&self, _repo_root: &Path) -> Result<()> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Uninstall);
        if let Some(message) = state.uninstall_failures.pop_front() {
            return Err(anyhow!("{message}"));
        }
        state.installed = None;
        Ok(())
    }

    fn is_loaded(&self, _repo_root: &Path) -> bool {
        let mut state = self.lock();
        state.calls.push(FakeCall::IsLoaded);
        state.installed.is_some()
    }
}

#[cfg(test)]
#[path = "fake_tests.rs"]
mod tests;
