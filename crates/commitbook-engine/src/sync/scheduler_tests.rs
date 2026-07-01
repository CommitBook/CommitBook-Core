use super::*;
use crate::ai::ConflictResolver;
use crate::git::test_support::{
    clone_second_workdir, commit_and_push_from, setup_repo_with_bare_remote, RepoFixture,
};
use crate::logger::FileLogger;
use crate::platform::SystemCredentials;
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

/// Mock conflict resolver that always returns the same string. Used to
/// verify that the orchestrator wires conflicted-file content through to a
/// resolver and writes the result back.
struct MockResolver {
    name: &'static str,
    output: String,
    fail: bool,
}

impl MockResolver {
    fn ok(output: impl Into<String>) -> Self {
        Self {
            name: "MockResolver",
            output: output.into(),
            fail: false,
        }
    }
    fn failing() -> Self {
        Self {
            name: "FailingMock",
            output: String::new(),
            fail: true,
        }
    }
}

#[async_trait]
impl ConflictResolver for MockResolver {
    fn name(&self) -> &str {
        self.name
    }
    fn key(&self) -> &str {
        "mock"
    }
    fn is_available(&self) -> bool {
        true
    }
    async fn resolve(
        &self,
        _file_path: &Path,
        _content_with_markers: &str,
        _repo_path: &Path,
    ) -> Result<String> {
        if self.fail {
            anyhow::bail!("mock failure");
        }
        Ok(self.output.clone())
    }
}

/// Build a `RepoFixture` and a sibling `.CommitBook/local/` directory so the
/// sync orchestrator can save state. Returns the FileLogger over that local
/// dir so test scaffolding stays compact.
fn setup_with_state() -> (RepoFixture, FileLogger) {
    let fx = setup_repo_with_bare_remote();
    let cb_dir = fx.repo_dir.path().join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local").join("logs")).unwrap();
    let logger = FileLogger::new(fx.repo_dir.path(), 30).unwrap();
    (fx, logger)
}

fn cb_dir_of(fx: &RepoFixture) -> std::path::PathBuf {
    fx.repo_dir.path().join(".CommitBook")
}

#[tokio::test]
async fn scenario_1_up_to_date_no_changes() {
    let (fx, logger) = setup_with_state();
    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(outcome.is_clean(), "expected clean outcome, got {outcome:?}");
}

#[tokio::test]
async fn scenario_2_local_only_edit_commits_and_pushes() {
    let (fx, logger) = setup_with_state();
    std::fs::write(fx.repo_dir.path().join("note.md"), "# note\n").unwrap();

    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        Some("Test commit".to_string()),
    )
    .await
    .unwrap();
    assert!(outcome.committed);
    assert_eq!(outcome.pushed, 1);
    assert_eq!(outcome.pulled, 0);
    assert!(outcome.errors.is_empty(), "{outcome:?}");
}

#[tokio::test]
async fn scenario_3_remote_only_changes_pulls() {
    let (fx, logger) = setup_with_state();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "# remote\n");

    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert_eq!(outcome.pulled, 1);
    assert_eq!(outcome.pushed, 0);
    assert!(!outcome.committed);
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert!(fx.repo_dir.path().join("remote.md").exists());
}

#[tokio::test]
async fn scenario_4_local_and_remote_different_files() {
    let (fx, logger) = setup_with_state();

    // Local: dirty edit on a different file.
    std::fs::write(fx.repo_dir.path().join("local.md"), "# local\n").unwrap();

    // Remote: a different file pushed by another client.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "# remote\n");

    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        Some("local edit".to_string()),
    )
    .await
    .unwrap();
    assert!(outcome.committed);
    assert_eq!(outcome.pushed, 1);
    assert_eq!(outcome.pulled, 1);
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert!(fx.repo_dir.path().join("remote.md").exists());
    assert!(fx.repo_dir.path().join("local.md").exists());
}

#[tokio::test]
async fn scenario_5_same_file_non_overlapping_lines() {
    let (fx, logger) = setup_with_state();

    // Establish a shared file at HEAD.
    let shared = "line A\nline B\nline C\n";
    std::fs::write(fx.repo_dir.path().join("shared.md"), shared).unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    let push = std::process::Command::new("git")
        .args(["push", "origin", &fx.branch])
        .current_dir(fx.repo_dir.path())
        .output()
        .unwrap();
    assert!(push.status.success());

    // Local: edit line A (uncommitted).
    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A LOCAL\nline B\nline C\n").unwrap();

    // Remote: edit line C.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(
        other.path(),
        &fx.branch,
        "shared.md",
        "line A\nline B\nline C REMOTE\n",
    );

    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        Some("merged edit".to_string()),
    )
    .await
    .unwrap();
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert_eq!(outcome.manual_conflicts, 0);
    let final_content = std::fs::read_to_string(fx.repo_dir.path().join("shared.md")).unwrap();
    assert!(final_content.contains("line A LOCAL"));
    assert!(final_content.contains("line C REMOTE"));
}

#[tokio::test]
async fn scenario_6_same_lines_manual_mode_returns_manual_conflicts() {
    let (fx, logger) = setup_with_state();

    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    let _ = std::process::Command::new("git")
        .args(["push", "origin", &fx.branch])
        .current_dir(fx.repo_dir.path())
        .output()
        .unwrap();

    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A LOCAL\n").unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "shared.md", "line A REMOTE\n");

    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None, // manual mode
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert_eq!(outcome.manual_conflicts, 1);
    assert!(!outcome.errors.is_empty());
    // Working tree should still contain the conflict markers.
    let content = std::fs::read_to_string(fx.repo_dir.path().join("shared.md")).unwrap();
    assert!(content.contains("<<<<<<<"));
}

#[tokio::test]
async fn scenario_7_same_lines_ai_mock_resolves() {
    let (fx, logger) = setup_with_state();

    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    let _ = std::process::Command::new("git")
        .args(["push", "origin", &fx.branch])
        .current_dir(fx.repo_dir.path())
        .output()
        .unwrap();

    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A LOCAL\n").unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "shared.md", "line A REMOTE\n");

    let resolver = MockResolver::ok("line A RESOLVED\n");
    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        Some(&resolver),
        &SystemCredentials,
        &logger,
        Some("merged via AI".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(outcome.conflicts_resolved, 1);
    assert_eq!(outcome.manual_conflicts, 0);
    assert!(outcome.errors.is_empty(), "{outcome:?}");

    let content = std::fs::read_to_string(fx.repo_dir.path().join("shared.md")).unwrap();
    assert_eq!(content.trim(), "line A RESOLVED");
    assert!(!content.contains("<<<<<<<"));
}

#[tokio::test]
async fn scenario_8_failing_resolver_falls_back_to_manual() {
    let (fx, logger) = setup_with_state();

    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    let _ = std::process::Command::new("git")
        .args(["push", "origin", &fx.branch])
        .current_dir(fx.repo_dir.path())
        .output()
        .unwrap();

    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A LOCAL\n").unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "shared.md", "line A REMOTE\n");

    let resolver = MockResolver::failing();
    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        Some(&resolver),
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert_eq!(outcome.conflicts_resolved, 0);
    assert_eq!(outcome.manual_conflicts, 1);
    assert!(!outcome.errors.is_empty());
}

#[tokio::test]
async fn scenario_9_unpushed_local_commits_rebase_onto_remote() {
    let (fx, logger) = setup_with_state();

    // Local: commit a file but don't push.
    std::fs::write(fx.repo_dir.path().join("local.md"), "# local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local commit").unwrap();
    let local_before = fx.repo.rev_parse("HEAD").unwrap();

    // Remote: another client pushed a different file.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "# remote\n");

    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    // After rebase, local HEAD should differ from before (new SHA after replay).
    let local_after = fx.repo.rev_parse("HEAD").unwrap();
    assert_ne!(local_before, local_after);
    // Both files should be present and pushed.
    assert!(fx.repo_dir.path().join("local.md").exists());
    assert!(fx.repo_dir.path().join("remote.md").exists());
}

#[tokio::test]
async fn scenario_10_pull_failure_surfaces_error() {
    let (fx, logger) = setup_with_state();
    // Use a remote name that doesn't exist to force a pull failure.
    let outcome = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "nonexistent-remote",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(!outcome.errors.is_empty());
    assert!(!outcome.committed);
    assert_eq!(outcome.pushed, 0);
}

#[test]
fn sync_outcome_default_is_clean() {
    let outcome = SyncOutcome::default();
    assert!(outcome.is_clean());
}
