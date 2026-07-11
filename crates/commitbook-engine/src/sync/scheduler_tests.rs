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

/// Set up a shared-file conflict and run one manual-mode cycle, leaving the
/// merge in progress with conflict markers in the working tree.
async fn leave_merge_in_progress(fx: &RepoFixture, logger: &FileLogger) {
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

    let first = sync_with_resolver(
        &cb_dir_of(fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None, // manual mode
        &SystemCredentials,
        logger,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first.manual_conflicts, 1);
    assert!(fx.repo.merge_in_progress());
}

#[tokio::test]
async fn resolver_returning_conflict_markers_falls_back_to_manual() {
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

    // The resolver "succeeds" but leaves conflict markers in its output; the
    // scheduler must reject it rather than commit the markers.
    let resolver =
        MockResolver::ok("<<<<<<< HEAD\nstill conflicted\n=======\nnope\n>>>>>>> other\n");
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
async fn scenario_11_merge_in_progress_unresolved_reports_manual() {
    let (fx, logger) = setup_with_state();
    leave_merge_in_progress(&fx, &logger).await;
    let head_before = fx.repo.rev_parse("HEAD").unwrap();

    // A second cycle recovers the in-progress merge; still unresolved.
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
    assert_eq!(outcome.manual_conflicts, 1);
    assert!(!outcome.committed);
    assert!(!outcome.errors.is_empty());
    // Nothing new was committed; the merge is still pending.
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
    assert!(fx.repo.merge_in_progress());
}

#[tokio::test]
async fn scenario_12_merge_in_progress_resolved_finalizes() {
    let (fx, logger) = setup_with_state();
    leave_merge_in_progress(&fx, &logger).await;

    // The user resolves the markers by hand and stages the file.
    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A RESOLVED\n").unwrap();
    fx.repo.stage_paths(&["shared.md".to_string()]).unwrap();

    // A second cycle finalizes the merge and pushes it.
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
    assert_eq!(outcome.manual_conflicts, 0);
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert!(outcome.pushed >= 1);
    assert!(!fx.repo.merge_in_progress());
}

#[tokio::test]
async fn failed_cycle_records_last_error_without_advancing_last_sync_at() {
    use crate::state::sync_state::SyncState;
    let (fx, logger) = setup_with_state();

    // A successful cycle stamps last_sync_at and clears last_error.
    std::fs::write(fx.repo_dir.path().join("note.md"), "# note\n").unwrap();
    let ok = sync_with_resolver(
        &cb_dir_of(&fx),
        fx.repo_dir.path(),
        "origin",
        &fx.branch,
        None,
        &SystemCredentials,
        &logger,
        Some("note".to_string()),
    )
    .await
    .unwrap();
    assert!(ok.errors.is_empty(), "{ok:?}");
    let state1 = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert!(state1.last_sync_at.is_some());
    assert!(state1.last_error.is_none());

    // Force a failing cycle: a same-line conflict with a failing resolver.
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
    let bad = sync_with_resolver(
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
    assert!(!bad.errors.is_empty());

    let state2 = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert!(state2.last_error.is_some(), "failure should record last_error");
    assert_eq!(
        state2.last_sync_at, state1.last_sync_at,
        "a failed cycle must not advance last_sync_at"
    );
}

#[tokio::test]
async fn first_sync_against_empty_remote_bootstraps_branch() {
    use git2::{Repository, Signature};

    // A bare remote with no branches yet.
    let remote_dir = tempfile::tempdir().unwrap();
    Repository::init_bare(remote_dir.path()).unwrap();

    // A local repo with one commit and origin set, but nothing pushed.
    let repo_dir = tempfile::tempdir().unwrap();
    let branch = {
        let repo = Repository::init(repo_dir.path()).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test User").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
            config.set_bool("commit.gpgsign", false).unwrap();
        }
        std::fs::write(repo_dir.path().join("init.md"), "# init\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("init.md")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        let branch = repo
            .head()
            .unwrap()
            .shorthand()
            .unwrap_or("main")
            .to_string();
        repo.remote(
            "origin",
            &format!("file://{}", remote_dir.path().display()),
        )
        .unwrap();
        branch
    };

    let cb_dir = repo_dir.path().join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local").join("logs")).unwrap();
    let logger = FileLogger::new(repo_dir.path(), 30).unwrap();

    let outcome = sync_with_resolver(
        &cb_dir,
        repo_dir.path(),
        "origin",
        &branch,
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert!(
        outcome.errors.is_empty(),
        "first sync against an empty remote should succeed: {outcome:?}"
    );
    // The bootstrap push is reported, not shown as "already up to date".
    assert!(outcome.pushed >= 1, "bootstrap push should count: {outcome:?}");
    assert!(!outcome.is_clean(), "a real bootstrap push is not a clean no-op");
    // The push bootstrapped the branch on the remote.
    let remote_repo = Repository::open_bare(remote_dir.path()).unwrap();
    assert!(
        remote_repo
            .refname_to_id(&format!("refs/heads/{branch}"))
            .is_ok(),
        "push should have created refs/heads/{branch} on the remote"
    );
}
