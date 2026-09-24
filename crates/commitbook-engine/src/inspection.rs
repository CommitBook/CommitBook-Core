//! Network-free inspection shared by all desktop interfaces.
use crate::{config::LocalConfig, git::GitRepo, state::sync_state::SyncState};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const INCLUSION_POLICY: &str = "Sync commits all tracked and non-ignored files, including non-Markdown files and deletions. Preview is a snapshot; files may change before sync.";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEntry {
    pub path: String,
    pub change: String,
    pub staged: bool,
    pub unstaged: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitPreview {
    pub repository: String,
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub policy: String,
    pub entries: Vec<PreviewEntry>,
    pub blockers: Vec<String>,
}
/// Inspect files even before initialization, for the init inclusion summary.
pub fn preview_files(root: &Path) -> Result<Vec<PreviewEntry>> {
    let repo = git2::Repository::open(root)?;
    let tree = match repo.head() {
        Ok(head) => Some(head.peel_to_tree()?),
        Err(e)
            if matches!(
                e.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            None
        }
        Err(e) => return Err(e.into()),
    };
    let mut options = git2::DiffOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_typechange(true)
        .include_unreadable(true)
        .update_index(false);
    // Direct tree/workdir comparison predicts git add -A, unlike blending
    // the index diff, which retains staged deletions recreated in the workdir.
    let diff = repo.diff_tree_to_workdir(tree.as_ref(), Some(&mut options))?;
    let mut entries = Vec::new();
    for delta in diff.deltas() {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .context("Change has no path")?;
        let path_str = path.to_str().context("Change path is not valid UTF-8")?;
        let status = repo.status_file(path)?;
        let change = match delta.status() {
            git2::Delta::Added | git2::Delta::Untracked => "added",
            git2::Delta::Deleted => "deleted",
            git2::Delta::Typechange => "type_changed",
            git2::Delta::Unreadable => anyhow::bail!("Cannot read {path_str}"),
            git2::Delta::Conflicted => "conflicted",
            git2::Delta::Unmodified | git2::Delta::Ignored => continue,
            _ => "modified",
        };
        entries.push(PreviewEntry {
            path: path_str.into(),
            change: change.into(),
            staged: status.intersects(
                git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED
                    | git2::Status::INDEX_TYPECHANGE,
            ),
            unstaged: status.intersects(
                git2::Status::WT_NEW
                    | git2::Status::WT_MODIFIED
                    | git2::Status::WT_DELETED
                    | git2::Status::WT_RENAMED
                    | git2::Status::WT_TYPECHANGE
                    | git2::Status::CONFLICTED,
            ),
        });
    }
    // Ignored paths need index-aware handling: force-staged additions remain
    // eligible, while staged deletions recreated as ignored files stay deleted.
    // Inspect only staged paths, avoiding scans through ignored build folders.
    let staged = repo.diff_tree_to_index(tree.as_ref(), None, None)?;
    for delta in staged.deltas() {
        if !matches!(delta.status(), git2::Delta::Added | git2::Delta::Deleted) {
            continue;
        }
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .context("Staged change has no path")?;
        if !repo.status_should_ignore(path)? {
            continue;
        }
        let name = path.to_str().context("Change path is not valid UTF-8")?;
        entries.retain(|e| e.path != name);
        let exists = match std::fs::symlink_metadata(root.join(path)) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if delta.status() == git2::Delta::Added && !exists {
            continue;
        }
        let status = repo.status_file(path)?;
        entries.push(PreviewEntry {
            path: name.into(),
            change: if delta.status() == git2::Delta::Added {
                "added"
            } else {
                "deleted"
            }
            .into(),
            staged: true,
            unstaged: status.intersects(
                git2::Status::WT_NEW
                    | git2::Status::WT_MODIFIED
                    | git2::Status::WT_DELETED
                    | git2::Status::WT_TYPECHANGE,
            ),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
pub fn preview(root: &Path) -> CommitPreview {
    let mut result = CommitPreview {
        repository: root.display().to_string(),
        branch: None,
        remote: None,
        policy: INCLUSION_POLICY.into(),
        entries: vec![],
        blockers: vec![],
    };
    match LocalConfig::load_read_only(root) {
        Ok(c) => {
            result.branch = Some(c.git.branch);
            result.remote = Some(c.git.remote);
        }
        Err(e) => result
            .blockers
            .push(format!("Cannot load configuration: {e:#}")),
    }
    match GitRepo::open(root) {
        Ok(repo) => {
            if repo.merge_in_progress() {
                result
                    .blockers
                    .push("Resolve the current merge before a normal snapshot".into());
            }
            match repo.current_branch() {
                Ok(branch) if result.branch.as_ref().is_some_and(|b| *b != branch) => result
                    .blockers
                    .push("Check out the configured branch before syncing".into()),
                Err(e) => result
                    .blockers
                    .push(format!("Cannot inspect branch: {e:#}")),
                _ => (),
            }
        }
        Err(e) => result
            .blockers
            .push(format!("Cannot inspect repository: {e:#}")),
    }
    match preview_files(root) {
        Ok(entries) => result.entries = entries,
        Err(e) => result
            .blockers
            .push(format!("Cannot preview changes: {e:#}")),
    }
    result
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepositoryStatus {
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub current_branch: Option<String>,
    pub schedule: Option<String>,
    pub head_oid: Option<String>,
    pub last_commit: Option<String>,
    pub head_timestamp: Option<String>,
    pub changes_total: Option<usize>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
    pub merge_in_progress: bool,
    pub conflicts: Vec<String>,
    pub pending_review: usize,
    pub last_attempt_at: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_fetch_at: Option<String>,
    pub last_push_at: Option<String>,
    pub last_error: Option<String>,
    pub last_error_stage: Option<String>,
    /// Notes where the last `both`-mode merge kept two versions, and when.
    pub kept_both_paths: Vec<String>,
    pub kept_both_at: Option<String>,
    pub devices: Vec<crate::devices::DeviceEntry>,
    pub diagnostics: Vec<String>,
    pub local_status: String,
    pub remote_status: String,
}
impl RepositoryStatus {
    pub fn read(root: &Path) -> Self {
        let mut s = Self::default();
        match LocalConfig::load_read_only(root) {
            Ok(c) => {
                s.branch = Some(c.git.branch);
                s.remote = Some(c.git.remote);
                s.schedule = Some(c.sync.schedule);
            }
            Err(e) => s
                .diagnostics
                .push(format!("Cannot load configuration: {e:#}")),
        }
        match SyncState::load(&LocalConfig::commitbook_dir(root)) {
            Ok(state) => {
                s.last_attempt_at = state.last_attempt_at;
                s.last_sync_at = state.last_sync_at;
                s.last_fetch_at = state.last_fetch_at;
                s.last_push_at = state.last_push_at;
                s.last_error = state.last_error;
                s.last_error_stage = state.last_error_stage;
                s.kept_both_paths = state.kept_both_paths;
                s.kept_both_at = state.kept_both_at;
            }
            Err(e) => s.diagnostics.push(format!("Cannot load sync state: {e:#}")),
        }
        match crate::devices::list(root) {
            Ok((devices, warnings)) => {
                s.devices = devices;
                s.diagnostics.extend(warnings);
            }
            Err(e) => s.diagnostics.push(format!("Cannot list devices: {e:#}")),
        }
        if let Err(e) = s.read_git(root) {
            s.diagnostics
                .push(format!("Cannot inspect repository: {e:#}"));
        }
        match crate::review::list(root) {
            Ok(conflicts) => {
                s.pending_review = conflicts
                    .iter()
                    .filter(|c| c.proposal.as_ref().is_some_and(|p| !p.rejected))
                    .count();
                s.conflicts = conflicts.into_iter().map(|c| c.path).collect();
            }
            Err(e) => s
                .diagnostics
                .push(format!("Cannot inspect conflicts: {e:#}")),
        }
        s.local_status = match s.changes_total {
            Some(n) if n > 0 => format!("Edits awaiting local commit ({n} files)"),
            Some(_) if s.head_oid.is_some() => "Latest edits committed locally".into(),
            Some(_) => "No local commits yet".into(),
            None => "Local save status unknown".into(),
        };
        s.remote_status = if s.pending_review > 0 {
            format!("AI resolution awaiting review ({} files)", s.pending_review)
        } else if s.merge_in_progress || !s.conflicts.is_empty() {
            "Conflict needs attention".into()
        } else if !s.diagnostics.is_empty() {
            "Remote status unknown; inspection needs attention".into()
        } else if s.ahead.is_some_and(|n| n > 0) {
            format!(
                "Waiting to upload ({} commits){}",
                s.ahead.unwrap(),
                if s.behind.is_some_and(|n| n > 0) {
                    "; remote updates available"
                } else {
                    ""
                }
            )
        } else if s.behind.is_some_and(|n| n > 0) {
            "Remote updates available at last remote check".into()
        } else if s.ahead == Some(0) && s.behind == Some(0) && s.changes_total == Some(0) {
            "Up to date at last remote check".into()
        } else {
            "Remote status unknown".into()
        };
        s
    }
    fn read_git(&mut self, root: &Path) -> Result<()> {
        let repo = git2::Repository::open(root)?;
        self.merge_in_progress = repo.state() == git2::RepositoryState::Merge;
        let mut options = git2::StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false)
            .update_index(false);
        self.changes_total = Some(repo.statuses(Some(&mut options))?.len());
        match repo.head() {
            Ok(head) => {
                self.current_branch = Some(head.shorthand()?.to_owned());
                let commit = head.peel_to_commit()?;
                self.head_oid = Some(commit.id().to_string());
                self.last_commit = Some(commit.summary()?.unwrap_or("(no summary)").into());
                self.head_timestamp = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
                    .map(|d| d.to_rfc3339());
                if !head.is_branch()
                    || (self.branch.is_some() && self.branch != self.current_branch)
                {
                    self.diagnostics
                        .push("Check out the configured branch before syncing".into());
                }
                if let (Some(remote), Some(branch)) = (&self.remote, &self.branch) {
                    match repo.find_reference(&format!("refs/remotes/{remote}/{branch}")) {
                        Ok(reference) => {
                            let remote = reference.peel_to_commit()?;
                            let (ahead, behind) =
                                repo.graph_ahead_behind(commit.id(), remote.id())?;
                            self.ahead = Some(ahead);
                            self.behind = Some(behind);
                        }
                        Err(e) if e.code() == git2::ErrorCode::NotFound => (),
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            Err(e)
                if matches!(
                    e.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                self.current_branch = repo
                    .find_reference("HEAD")?
                    .symbolic_target()?
                    .and_then(|s| s.strip_prefix("refs/heads/"))
                    .map(str::to_owned);
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            self.local_status.clone(),
            self.remote_status.clone(),
            format!(
                "Last commit: {}",
                self.last_commit.as_deref().unwrap_or("none")
            ),
            format!(
                "Last remote check: {}",
                self.last_fetch_at.as_deref().unwrap_or("unknown")
            ),
            format!(
                "Last successful push: {}",
                self.last_push_at.as_deref().unwrap_or("unknown")
            ),
        ];
        if !self.devices.is_empty() {
            let names: Vec<&str> = self
                .devices
                .iter()
                .map(|d| d.device.name.as_str())
                .collect();
            lines.push(format!(
                "Devices: {} ({})",
                self.devices.len(),
                names.join(", ")
            ));
        }
        if !self.kept_both_paths.is_empty() {
            lines.push(format!(
                "Both versions kept{}; delete the one you don't want in: {}",
                self.kept_both_at
                    .as_deref()
                    .map(|at| format!(" at {at}"))
                    .unwrap_or_default(),
                self.kept_both_paths.join(", ")
            ));
        }
        if let Some(e) = &self.last_error {
            lines.push(format!(
                "Last error ({}): {e}",
                self.last_error_stage.as_deref().unwrap_or("sync")
            ));
        }
        lines.extend(self.diagnostics.clone());
        if !self.conflicts.is_empty() {
            lines.push(format!(
                "Resolve in the web dashboard /conflicts, or edit, git add, and sync: {}",
                self.conflicts.join(", ")
            ));
        }
        lines
    }
}
#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;
