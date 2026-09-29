use super::*;
use crate::ai::{ConflictResolution, ConflictResolver};
use crate::git::test_support::{
    clone_second_workdir, commit_and_push_from, set_repo_excludes, setup_repo_with_bare_remote,
    RepoFixture,
};
use crate::git::GitConflict;
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
        _conflict: &GitConflict,
        _repo_path: &Path,
    ) -> Result<ConflictResolution> {
        if self.fail {
            anyhow::bail!("mock failure");
        }
        Ok(ConflictResolution::WriteContent(self.output.clone()))
    }
}

struct PartialFailureResolver;

#[async_trait]
impl ConflictResolver for PartialFailureResolver {
    fn name(&self) -> &str {
        "PartialFailureResolver"
    }

    fn key(&self) -> &str {
        "partial-failure"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn resolve(
        &self,
        conflict: &GitConflict,
        _repo_path: &Path,
    ) -> Result<ConflictResolution> {
        if conflict.path == "a.md" {
            Ok(ConflictResolution::WriteContent("A RESOLVED\n".to_string()))
        } else {
            anyhow::bail!("intentional failure after one resolution")
        }
    }
}

/// Build a `RepoFixture` and a sibling `.CommitBook/local/` directory so the
/// sync orchestrator can save state. Returns the FileLogger over that local
/// dir so test scaffolding stays compact.
fn setup_with_state() -> (RepoFixture, FileLogger) {
    let fx = setup_repo_with_bare_remote();
    let cb_dir = fx.repo_dir.path().join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local").join("logs")).unwrap();
    std::fs::write(
        fx.repo_dir.path().join(".git/info/exclude"),
        ".CommitBook/local/\n",
    )
    .unwrap();
    // Commit and publish the ignore file `init` writes, since sync restores it
    // when it is missing.
    LocalConfig::ensure_gitignore(fx.repo_dir.path()).unwrap();
    fx.repo
        .stage_paths(&[".CommitBook/.gitignore".to_string()])
        .unwrap();
    fx.repo.commit("add .CommitBook/.gitignore").unwrap();
    let pushed = std::process::Command::new("git")
        .args(["push", "-q", "origin", &fx.branch])
        .current_dir(fx.repo_dir.path())
        .output()
        .unwrap();
    assert!(pushed.status.success());
    let logger = FileLogger::new(fx.repo_dir.path(), crate::config::LogKeep::Days(30)).unwrap();
    (fx, logger)
}

fn cb_dir_of(fx: &RepoFixture) -> std::path::PathBuf {
    fx.repo_dir.path().join(".CommitBook")
}

#[tokio::test]
async fn scenario_1_up_to_date_no_changes() {
    let (fx, logger) = setup_with_state();
    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(
        outcome.is_clean(),
        "expected clean outcome, got {outcome:?}"
    );
}

#[tokio::test]
async fn scenario_2_local_only_edit_commits_and_pushes() {
    let (fx, logger) = setup_with_state();
    std::fs::write(fx.repo_dir.path().join("note.md"), "# note\n").unwrap();

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        Some("Test commit".to_string()),
    )
    .await
    .unwrap();
    assert!(outcome.committed);
    let repo = git2::Repository::open(fx.repo_dir.path()).unwrap();
    assert_eq!(
        repo.head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .message()
            .unwrap(),
        "Test commit"
    );
    assert_eq!(outcome.pushed, 1);
    assert_eq!(outcome.pulled, 0);
    assert!(outcome.errors.is_empty(), "{outcome:?}");
}

#[tokio::test]
async fn sync_commits_every_nonignored_git_change_and_pushes_exact_tree() {
    let (fx, logger) = setup_with_state();

    // Establish a tracked path so the cycle also has a deletion to stage.
    std::fs::write(fx.repo_dir.path().join("delete.txt"), "remove me\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add deletion fixture").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    std::fs::write(fx.repo_dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(fx.repo_dir.path().join("ignored.txt"), "private\n").unwrap();
    std::fs::create_dir_all(fx.repo_dir.path().join(".hidden")).unwrap();
    std::fs::write(fx.repo_dir.path().join(".hidden/data.bin"), b"hidden").unwrap();
    std::fs::write(fx.repo_dir.path().join("data.json"), "{}\n").unwrap();
    std::fs::write(fx.repo_dir.path().join("already-staged.txt"), "staged\n").unwrap();
    fx.repo
        .stage_paths(&["already-staged.txt".to_string()])
        .unwrap();
    std::fs::remove_file(fx.repo_dir.path().join("delete.txt")).unwrap();

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        Some("commit every Git change".to_string()),
    )
    .await
    .unwrap();

    assert!(outcome.committed, "{outcome:?}");
    assert_eq!(outcome.pushed, 1, "{outcome:?}");
    assert!(outcome.errors.is_empty(), "{outcome:?}");

    let remote = git2::Repository::open_bare(fx.remote_dir.path()).unwrap();
    let tree = remote
        .find_commit(
            remote
                .refname_to_id(&format!("refs/heads/{}", fx.branch))
                .unwrap(),
        )
        .unwrap()
        .tree()
        .unwrap();
    for path in [
        ".gitignore",
        ".hidden/data.bin",
        "data.json",
        "already-staged.txt",
    ] {
        assert!(tree.get_path(Path::new(path)).is_ok(), "missing {path}");
    }
    assert!(tree.get_path(Path::new("delete.txt")).is_err());
    assert!(tree.get_path(Path::new("ignored.txt")).is_err());
    assert!(fx.repo_dir.path().join("ignored.txt").exists());
}

#[tokio::test]
async fn auto_push_false_commits_and_merges_without_publishing() {
    let (fx, logger) = setup_with_state();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote-only.md", "remote\n");
    let remote_before = git2::Repository::open_bare(fx.remote_dir.path())
        .unwrap()
        .refname_to_id(&format!("refs/heads/{}", fx.branch))
        .unwrap();
    std::fs::create_dir_all(fx.repo_dir.path().join(".hidden")).unwrap();
    std::fs::write(fx.repo_dir.path().join(".hidden/data.txt"), "local\n").unwrap();

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, false),
        None,
        &SystemCredentials,
        &logger,
        Some("local only".to_string()),
    )
    .await
    .unwrap();

    assert!(outcome.committed, "{outcome:?}");
    assert!(outcome.pulled >= 1, "{outcome:?}");
    assert_eq!(outcome.pushed, 0);
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert_eq!(
        fx.repo
            .show_file_at_ref("HEAD", ".hidden/data.txt")
            .unwrap(),
        "local\n"
    );
    assert_eq!(
        fx.repo.show_file_at_ref("HEAD", "remote-only.md").unwrap(),
        "remote\n"
    );
    let remote_after = git2::Repository::open_bare(fx.remote_dir.path())
        .unwrap()
        .refname_to_id(&format!("refs/heads/{}", fx.branch))
        .unwrap();
    assert_eq!(remote_before, remote_after);
    let remote = git2::Repository::open_bare(fx.remote_dir.path()).unwrap();
    let remote_tree = remote.find_commit(remote_after).unwrap().tree().unwrap();
    assert!(remote_tree.get_path(Path::new("remote-only.md")).is_ok());
    assert!(remote_tree.get_path(Path::new(".hidden/data.txt")).is_err());
}

#[tokio::test]
async fn scenario_3_remote_only_changes_pulls() {
    let (fx, logger) = setup_with_state();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "# remote\n");

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
    std::fs::write(
        fx.repo_dir.path().join("shared.md"),
        "line A LOCAL\nline B\nline C\n",
    )
    .unwrap();

    // Remote: edit line C.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(
        other.path(),
        &fx.branch,
        "shared.md",
        "line A\nline B\nline C REMOTE\n",
    );

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
async fn partial_resolver_failure_reports_resolved_and_remaining_counts() {
    let (fx, logger) = setup_with_state();
    std::fs::write(fx.repo_dir.path().join("a.md"), "A BASE\n").unwrap();
    std::fs::write(fx.repo_dir.path().join("b.md"), "B BASE\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add conflict fixtures").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    std::fs::write(other.path().join("a.md"), "A REMOTE\n").unwrap();
    std::fs::write(other.path().join("b.md"), "B REMOTE\n").unwrap();
    let other_repo = crate::git::GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote conflicts").unwrap();
    other_repo.push("origin", &fx.branch).unwrap();

    std::fs::write(fx.repo_dir.path().join("a.md"), "A LOCAL\n").unwrap();
    std::fs::write(fx.repo_dir.path().join("b.md"), "B LOCAL\n").unwrap();
    let resolver = PartialFailureResolver;
    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        Some(&resolver),
        &SystemCredentials,
        &logger,
        Some("local conflicts".to_string()),
    )
    .await
    .unwrap();

    assert_eq!(outcome.conflicts_resolved, 1, "{outcome:?}");
    assert_eq!(outcome.manual_conflicts, 1, "{outcome:?}");
    assert_eq!(fx.repo.list_conflicted_paths().unwrap(), ["b.md"]);
    assert_eq!(
        std::fs::read_to_string(fx.repo_dir.path().join("a.md")).unwrap(),
        "A RESOLVED\n"
    );
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("nonexistent-remote", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
async fn retry_with_resolver_completes_a_preserved_manual_merge() {
    let (fx, logger) = setup_with_state();
    leave_merge_in_progress(&fx, &logger).await;

    let resolver = MockResolver::ok("line A RESOLVED ON RETRY\n");
    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        Some(&resolver),
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert_eq!(outcome.conflicts_resolved, 1, "{outcome:?}");
    assert_eq!(outcome.manual_conflicts, 0, "{outcome:?}");
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert!(!fx.repo.merge_in_progress());
    assert_eq!(
        std::fs::read_to_string(fx.repo_dir.path().join("shared.md")).unwrap(),
        "line A RESOLVED ON RETRY\n"
    );
    let remote = git2::Repository::open_bare(fx.remote_dir.path()).unwrap();
    let tree = remote
        .find_reference(&format!("refs/heads/{}", fx.branch))
        .unwrap()
        .peel_to_tree()
        .unwrap();
    let entry = tree.get_path(Path::new("shared.md")).unwrap();
    let blob = remote.find_blob(entry.id()).unwrap();
    assert_eq!(blob.content(), b"line A RESOLVED ON RETRY\n");
}

#[tokio::test]
async fn sync_refuses_a_checkout_different_from_the_configured_branch() {
    let (fx, logger) = setup_with_state();
    let configured_branch = fx.branch.clone();
    let repo = git2::Repository::open(fx.repo_dir.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("other", &head, false).unwrap();
    repo.set_head("refs/heads/other").unwrap();
    repo.checkout_head(None).unwrap();
    drop(head);
    drop(repo);
    let head_before = fx.repo.rev_parse("HEAD").unwrap();

    let error = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &configured_branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(error.contains("configured for"));
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
}

#[test]
fn push_retry_classification_only_accepts_non_fast_forward() {
    let non_fast_forward = anyhow::Error::new(git2::Error::new(
        git2::ErrorCode::NotFastForward,
        git2::ErrorClass::Net,
        "remote advanced",
    ));
    let auth = anyhow::Error::new(git2::Error::new(
        git2::ErrorCode::Auth,
        git2::ErrorClass::Net,
        "bad credentials",
    ));
    let diagnostic_text = anyhow::anyhow!("non-fast-forward appears in unrelated diagnostics");

    assert!(is_non_fast_forward_push(&non_fast_forward));
    assert!(!is_non_fast_forward_push(&auth));
    assert!(!is_non_fast_forward_push(&diagnostic_text));
}

#[tokio::test]
async fn server_rejection_is_not_retried() {
    let (fx, logger) = setup_with_state();
    std::fs::write(fx.repo_dir.path().join("local.txt"), "local\n").unwrap();
    crate::git::operations::set_push_failpoint(crate::git::operations::PushFailpoint::Auth);

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        Some("rejected push".to_string()),
    )
    .await
    .unwrap();

    assert_eq!(outcome.pushed, 0, "{outcome:?}");
    assert_eq!(outcome.errors.len(), 1, "{outcome:?}");
    assert_eq!(crate::git::operations::push_attempts(), 1);
}

#[tokio::test]
async fn non_fast_forward_push_is_retried_once() {
    let (fx, logger) = setup_with_state();
    std::fs::write(fx.repo_dir.path().join("local.txt"), "local\n").unwrap();
    crate::git::operations::set_push_failpoint(
        crate::git::operations::PushFailpoint::NonFastForward,
    );

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        Some("retry push".to_string()),
    )
    .await
    .unwrap();

    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert_eq!(outcome.pushed, 1, "{outcome:?}");
    assert_eq!(crate::git::operations::push_attempts(), 2);
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
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
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        Some(&resolver),
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(!bad.errors.is_empty());

    let state2 = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert!(
        state2.last_error.is_some(),
        "failure should record last_error"
    );
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
        let branch = repo.head().unwrap().shorthand().unwrap().to_string();
        repo.remote("origin", &format!("file://{}", remote_dir.path().display()))
            .unwrap();
        branch
    };

    let cb_dir = repo_dir.path().join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local").join("logs")).unwrap();
    std::fs::write(
        repo_dir.path().join(".git/info/exclude"),
        ".CommitBook/local/\n",
    )
    .unwrap();
    let logger = FileLogger::new(repo_dir.path(), crate::config::LogKeep::Days(30)).unwrap();

    let outcome = sync_with_resolver(
        repo_dir.path(),
        &SyncOptions::new("origin", &branch, true),
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
    assert!(
        outcome.pushed >= 1,
        "bootstrap push should count: {outcome:?}"
    );
    assert!(
        !outcome.is_clean(),
        "a real bootstrap push is not a clean no-op"
    );
    // The push bootstrapped the branch on the remote.
    let remote_repo = Repository::open_bare(remote_dir.path()).unwrap();
    assert!(
        remote_repo
            .refname_to_id(&format!("refs/heads/{branch}"))
            .is_ok(),
        "push should have created refs/heads/{branch} on the remote"
    );
}

#[tokio::test]
async fn default_commit_message_uses_writing_timestamp() {
    let (fx, logger) = setup_with_state();
    std::fs::write(fx.repo_dir.path().join("note.md"), "# note\n").unwrap();
    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(outcome.committed);
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    let repo = git2::Repository::open(fx.repo_dir.path()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    let message = commit.message().unwrap();
    assert_eq!(message.len(), "Writing YYYY-MM-DD HH:MM:SS".len());
    chrono::NaiveDateTime::parse_from_str(
        message.strip_prefix("Writing ").unwrap(),
        "%Y-%m-%d %H:%M:%S",
    )
    .unwrap();
}

fn record_pending_metadata(fx: &RepoFixture) -> String {
    crate::config::LocalConfig::new("notes", &fx.branch, "origin")
        .save(fx.repo_dir.path())
        .unwrap();
    crate::config::LocalConfig::ensure_gitignore(fx.repo_dir.path()).unwrap();
    fx.repo
        .commit_selected_paths_on_branch(
            &[".CommitBook/config.toml", ".CommitBook/.gitignore"],
            "Initialize CommitBook",
            &fx.branch,
        )
        .unwrap()
        .expect("metadata commit");
    let head = fx.repo.rev_parse("HEAD").unwrap();
    let mut state = SyncState::load(&cb_dir_of(fx)).unwrap();
    state.pending_init_push = Some(crate::state::sync_state::PendingInitPush {
        commit_oid: head.clone(),
        remote: "origin".to_string(),
        branch: fx.branch.clone(),
    });
    state.save(&cb_dir_of(fx)).unwrap();
    head
}

#[tokio::test]
async fn sync_push_clears_pending_init_push_once_remote_contains_it() {
    let (fx, logger) = setup_with_state();
    let head = record_pending_metadata(&fx);

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert!(outcome.pushed >= 1, "{outcome:?}");
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert!(SyncState::load(&cb_dir_of(&fx))
        .unwrap()
        .pending_init_push
        .is_none());
    let remote_tip = git2::Repository::open_bare(fx.remote_dir.path())
        .unwrap()
        .refname_to_id(&format!("refs/heads/{}", fx.branch))
        .unwrap();
    assert_eq!(remote_tip.to_string(), head);
}

#[tokio::test]
async fn sync_without_auto_push_keeps_pending_init_push() {
    let (fx, logger) = setup_with_state();
    let head = record_pending_metadata(&fx);

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, false),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert_eq!(outcome.pushed, 0, "{outcome:?}");
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert_eq!(
        SyncState::load(&cb_dir_of(&fx))
            .unwrap()
            .pending_init_push
            .unwrap()
            .commit_oid,
        head
    );
}

#[tokio::test]
async fn failure_stages_and_timestamps_preserve_existing_state() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    let cb_dir = cb_dir_of(&fx);
    let mut state = SyncState::default();
    state.last_sync_at = Some("previous successful cycle".into());
    state.pending_init_push = Some(crate::state::sync_state::PendingInitPush {
        commit_oid: fx.repo.rev_parse("HEAD").unwrap(),
        remote: "origin".into(),
        branch: fx.branch.clone(),
    });
    state.save(&cb_dir).unwrap();
    std::fs::write(root.join("offline.md"), "saved before fetch failure").unwrap();
    let outcome = sync_with_resolver(
        root,
        &SyncOptions::new("missing", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(outcome.committed);
    assert!(!outcome.errors.is_empty());
    let saved = SyncState::load(&cb_dir).unwrap();
    assert_eq!(saved.last_error_stage.as_deref(), Some("fetch"));
    assert!(saved.last_attempt_at.is_some());
    assert!(saved.last_fetch_at.is_none());
    assert!(saved.last_push_at.is_none());
    assert_eq!(saved.pending_init_push, state.pending_init_push);
    assert_eq!(saved.last_sync_at, state.last_sync_at);
    let outcome = sync_with_resolver(
        root,
        &SyncOptions::new("origin", &fx.branch, false),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(outcome.errors.is_empty());
    let saved = SyncState::load(&cb_dir).unwrap();
    assert!(saved.last_fetch_at.is_some());
    assert!(saved.last_push_at.is_none());
    assert!(saved.last_error.is_none());
    assert_eq!(saved.pending_init_push, state.pending_init_push);
}

#[tokio::test]
async fn push_failure_records_push_stage_and_preserves_pending_init() {
    use crate::state::sync_state::SyncState;
    let (fx, logger) = setup_with_state();
    let cb_dir = cb_dir_of(&fx);
    let mut state = SyncState::default();
    state.pending_init_push = Some(crate::state::sync_state::PendingInitPush {
        commit_oid: fx.repo.rev_parse("HEAD").unwrap(),
        remote: "origin".into(),
        branch: fx.branch.clone(),
    });
    state.save(&cb_dir).unwrap();
    std::fs::write(fx.repo_dir.path().join("local.txt"), "local\n").unwrap();
    crate::git::operations::set_push_failpoint(crate::git::operations::PushFailpoint::Auth);

    let outcome = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        Some("rejected push".to_string()),
    )
    .await
    .unwrap();

    assert_eq!(outcome.pushed, 0, "{outcome:?}");
    assert!(!outcome.errors.is_empty(), "{outcome:?}");
    let saved = SyncState::load(&cb_dir).unwrap();
    assert_eq!(saved.last_error_stage.as_deref(), Some("push"));
    assert!(saved
        .last_error
        .as_ref()
        .is_some_and(|e| e.contains("Push")));
    assert!(saved.last_fetch_at.is_some());
    assert!(saved.last_push_at.is_none());
    assert_eq!(saved.pending_init_push, state.pending_init_push);
}

#[tokio::test]
async fn merge_finalization_failure_is_labeled_merge_not_ai_resolution() {
    use crate::state::sync_state::SyncState;
    let (fx, logger) = setup_with_state();
    leave_merge_in_progress(&fx, &logger).await;
    std::fs::write(fx.repo_dir.path().join("shared.md"), "line A RESOLVED\n").unwrap();
    fx.repo.stage_paths(&["shared.md".to_string()]).unwrap();
    let raw = git2::Repository::open(fx.repo_dir.path()).unwrap();
    let mut config = raw.config().unwrap();
    config.set_bool("commit.gpgsign", true).unwrap();
    config
        .set_str("gpg.format", "commitbook-test-unsupported")
        .unwrap();

    let result = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await;
    assert!(result.is_err(), "{result:?}");
    let saved = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert_eq!(saved.last_error_stage.as_deref(), Some("merge"));
    assert_ne!(saved.last_error_stage.as_deref(), Some("ai_resolution"));
    assert!(saved.last_error.is_some());
    assert!(fx.repo.merge_in_progress());
}

#[tokio::test]
async fn ai_resolution_then_finalize_failure_is_still_labeled_merge() {
    use crate::state::sync_state::SyncState;
    let (fx, logger) = setup_with_state();
    leave_merge_in_progress(&fx, &logger).await;
    let raw = git2::Repository::open(fx.repo_dir.path()).unwrap();
    let mut config = raw.config().unwrap();
    config.set_bool("commit.gpgsign", true).unwrap();
    config
        .set_str("gpg.format", "commitbook-test-unsupported")
        .unwrap();
    let resolver = MockResolver::ok("line A RESOLVED\n");

    let result = sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        Some(&resolver),
        &SystemCredentials,
        &logger,
        None,
    )
    .await;
    assert!(result.is_err(), "{result:?}");
    let saved = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert_eq!(
        saved.last_error_stage.as_deref(),
        Some("merge"),
        "finalize after AI write must not remain ai_resolution"
    );
    assert!(fx.repo.merge_in_progress());
}

#[tokio::test]
async fn malformed_state_is_quarantined_and_commit_failure_is_recorded() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    let path = cb_dir_of(&fx).join("local/state.toml");
    std::fs::write(&path, "broken = [").unwrap();
    let head = fx.repo.rev_parse("HEAD").unwrap();
    std::fs::write(root.join("note.md"), "new text").unwrap();
    // A corrupt state file no longer stops sync; its content is kept aside.
    let outcome = sync_with_resolver(
        root,
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(outcome.committed, "{outcome:?}");
    assert_ne!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert_eq!(
        std::fs::read_to_string(cb_dir_of(&fx).join("local/state.toml.corrupt")).unwrap(),
        "broken = ["
    );
    let fresh = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert!(fresh.last_sync_at.is_some());
    std::fs::write(root.join("note.md"), "newer text").unwrap();
    // Unsupported signing format fails before invoking any external signing program.
    let raw = git2::Repository::open(root).unwrap();
    let mut config = raw.config().unwrap();
    config.set_bool("commit.gpgsign", true).unwrap();
    config
        .set_str("gpg.format", "commitbook-test-unsupported")
        .unwrap();
    let result = sync_with_resolver(
        root,
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await;
    assert!(result.is_err());
    let saved = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert_eq!(saved.last_error_stage.as_deref(), Some("commit"));
    assert!(saved.last_error.is_some());
}

/// Commit and push `base` for each file, then leave `local` edits
/// uncommitted and push `remote` edits from a second clone.
fn diverge_files(fx: &RepoFixture, files: &[(&str, &str, &str, &str)]) {
    for (path, base, _, _) in files {
        std::fs::write(fx.repo_dir.path().join(path), base).unwrap();
    }
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    for (path, _, local, remote) in files {
        std::fs::write(fx.repo_dir.path().join(path), local).unwrap();
        std::fs::write(other.path().join(path), remote).unwrap();
    }
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote edits").unwrap();
    other_repo.push("origin", &fx.branch).unwrap();
}

const APPEND_BASE: &str = "# Notes\n\n- first\n";
const APPEND_LOCAL: &str = "# Notes\n\n- first\n- local idea\n";
const APPEND_REMOTE: &str = "# Notes\n\n- first\n- remote idea\n";

fn options_with_keep_both(fx: &RepoFixture, enabled: bool) -> SyncOptions {
    let mut options = SyncOptions::new("origin", &fx.branch, true);
    options.keep_both = enabled;
    options
}

fn remote_tip(fx: &RepoFixture) -> String {
    fx.repo.fetch("origin", &fx.branch).unwrap();
    fx.repo.rev_parse(&format!("origin/{}", fx.branch)).unwrap()
}

async fn sync_keep_both(fx: &RepoFixture, logger: &FileLogger, enabled: bool) -> SyncOutcome {
    sync_with_resolver(
        fx.repo_dir.path(),
        &options_with_keep_both(fx, enabled),
        None,
        &SystemCredentials,
        logger,
        None,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn both_mode_keeps_both_versions_of_an_edited_line_and_pushes() {
    let (fx, logger) = setup_with_state();
    diverge_files(
        &fx,
        &[(
            "notes.md",
            "intro\nMeeting at 3pm\n",
            "intro\nMeeting at 4pm\n",
            "intro\nMeeting at 5pm\n",
        )],
    );

    let outcome = sync_keep_both(&fx, &logger, true).await;

    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert_eq!(outcome.kept_both, vec!["notes.md"]);
    assert_eq!(outcome.manual_conflicts, 0);
    assert!(!fx.repo.merge_in_progress());
    assert_eq!(
        std::fs::read_to_string(fx.repo_dir.path().join("notes.md")).unwrap(),
        "intro\nMeeting at 4pm\nMeeting at 5pm\n"
    );
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), remote_tip(&fx));
    let state = SyncState::load(&cb_dir_of(&fx)).unwrap();
    assert!(state.last_error.is_none());
    assert!(state.last_sync_at.is_some());
    assert_eq!(state.kept_both_paths, vec!["notes.md"]);
    assert!(state.kept_both_at.is_some());
}

#[tokio::test]
async fn both_mode_keeps_both_additions() {
    let (fx, logger) = setup_with_state();
    diverge_files(
        &fx,
        &[("notes.md", APPEND_BASE, APPEND_LOCAL, APPEND_REMOTE)],
    );

    let outcome = sync_keep_both(&fx, &logger, true).await;

    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert_eq!(outcome.kept_both, vec!["notes.md"]);
    assert_eq!(
        std::fs::read_to_string(fx.repo_dir.path().join("notes.md")).unwrap(),
        "# Notes\n\n- first\n- local idea\n- remote idea\n"
    );
}

#[tokio::test]
async fn manual_mode_leaves_markers_even_for_additions() {
    let (fx, logger) = setup_with_state();
    diverge_files(
        &fx,
        &[("notes.md", APPEND_BASE, APPEND_LOCAL, APPEND_REMOTE)],
    );

    let outcome = sync_keep_both(&fx, &logger, false).await;

    assert!(outcome.kept_both.is_empty());
    assert_eq!(outcome.manual_conflicts, 1);
    assert!(fx.repo.merge_in_progress());
}

#[tokio::test]
async fn both_mode_leaves_structured_files_for_manual_resolution() {
    let (fx, logger) = setup_with_state();
    diverge_files(
        &fx,
        &[
            ("notes.md", APPEND_BASE, APPEND_LOCAL, APPEND_REMOTE),
            (
                "settings.json",
                "{\"a\": 1}\n",
                "{\"a\": 2}\n",
                "{\"a\": 3}\n",
            ),
        ],
    );

    let outcome = sync_keep_both(&fx, &logger, true).await;

    assert_eq!(outcome.kept_both, vec!["notes.md"]);
    assert_eq!(outcome.manual_conflicts, 1);
    assert!(outcome.errors[0].contains("settings.json"), "{outcome:?}");
    assert!(fx.repo.merge_in_progress());
    assert_eq!(
        fx.repo.list_conflicted_paths().unwrap(),
        vec!["settings.json"]
    );
}

#[tokio::test]
async fn preserved_merge_is_completed_by_both_mode_on_next_cycle() {
    let (fx, logger) = setup_with_state();
    diverge_files(
        &fx,
        &[("notes.md", APPEND_BASE, APPEND_LOCAL, APPEND_REMOTE)],
    );
    let first = sync_keep_both(&fx, &logger, false).await;
    assert_eq!(first.manual_conflicts, 1);

    let second = sync_keep_both(&fx, &logger, true).await;

    assert!(second.errors.is_empty(), "{second:?}");
    assert_eq!(second.kept_both, vec!["notes.md"]);
    assert!(!fx.repo.merge_in_progress());
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), remote_tip(&fx));
}

#[tokio::test]
async fn config_driven_sync_registers_devices_that_every_clone_can_see() {
    let (fx, _) = setup_with_state();
    let config = crate::config::LocalConfig::new("notes", &fx.branch, "origin");
    config.save(fx.repo_dir.path()).unwrap();
    crate::config::LocalConfig::ensure_gitignore(fx.repo_dir.path()).unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("config").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    std::fs::create_dir_all(other.path().join(".CommitBook/local/logs")).unwrap();

    for root in [fx.repo_dir.path(), other.path(), fx.repo_dir.path()] {
        let logger = FileLogger::new(root, crate::config::LogKeep::Days(30)).unwrap();
        let outcome = sync_repository(root, &config, &logger, None).await.unwrap();
        assert!(outcome.errors.is_empty(), "{outcome:?}");
    }

    for root in [fx.repo_dir.path(), other.path()] {
        let (devices, warnings) = crate::devices::list(root).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(devices.len(), 2, "{devices:?}");
        assert_eq!(devices.iter().filter(|d| d.this_device).count(), 1);
    }
}

/// Run git in `dir` with a fixed identity; returns whether it succeeded so
/// commands expected to stop on a conflict can be run too.
fn git_in(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success()
}

/// Commit `content` to `notes.md` on the checked-out branch.
fn commit_notes(fx: &RepoFixture, content: &str, message: &str) {
    std::fs::write(fx.repo_dir.path().join("notes.md"), content).unwrap();
    assert!(git_in(fx.repo_dir.path(), &["add", "notes.md"]));
    assert!(git_in(fx.repo_dir.path(), &["commit", "-q", "-m", message]));
}

/// Sync with `both` mode on, as the default config does.
async fn try_sync_keep_both(fx: &RepoFixture, logger: &FileLogger) -> Result<SyncOutcome> {
    let mut options = SyncOptions::new("origin", &fx.branch, true);
    options.keep_both = true;
    sync_with_resolver(
        fx.repo_dir.path(),
        &options,
        None,
        &SystemCredentials,
        logger,
        None,
    )
    .await
}

fn remote_branch_oid(fx: &RepoFixture) -> git2::Oid {
    git2::Repository::open_bare(fx.remote_dir.path())
        .unwrap()
        .refname_to_id(&format!("refs/heads/{}", fx.branch))
        .unwrap()
}

/// Assert sync refused with `expected` in the error and changed nothing:
/// HEAD, the remote, and the conflicted file's markers are untouched.
async fn assert_refused(fx: &RepoFixture, logger: &FileLogger, expected: &str) {
    let head = fx.repo.rev_parse("HEAD").unwrap();
    let remote = remote_branch_oid(fx);
    let before = std::fs::read_to_string(fx.repo_dir.path().join("notes.md")).unwrap();
    let error = try_sync_keep_both(fx, logger).await.unwrap_err();
    assert!(format!("{error:#}").contains(expected), "{error:#}");
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert_eq!(remote_branch_oid(fx), remote);
    assert_eq!(
        std::fs::read_to_string(fx.repo_dir.path().join("notes.md")).unwrap(),
        before
    );
}

#[tokio::test]
async fn conflicted_stash_pop_is_not_committed() {
    let (fx, logger) = setup_with_state();
    commit_notes(&fx, "base\n", "base");
    std::fs::write(fx.repo_dir.path().join("notes.md"), "stashed\n").unwrap();
    assert!(git_in(fx.repo_dir.path(), &["stash", "-q"]));
    commit_notes(&fx, "committed\n", "diverge");
    assert!(!git_in(fx.repo_dir.path(), &["stash", "pop", "-q"]));
    assert_eq!(fx.repo.repository_state(), git2::RepositoryState::Clean);

    assert_refused(&fx, &logger, "unresolved conflicts left by a git command").await;
    assert!(std::fs::read_to_string(fx.repo_dir.path().join("notes.md"))
        .unwrap()
        .contains("<<<<<<<"));
}

#[tokio::test]
async fn conflicted_cherry_pick_is_not_committed() {
    let (fx, logger) = setup_with_state();
    commit_notes(&fx, "base\n", "base");
    assert!(git_in(
        fx.repo_dir.path(),
        &["checkout", "-q", "-b", "side"]
    ));
    commit_notes(&fx, "side\n", "side edit");
    let side = fx.repo.rev_parse("HEAD").unwrap();
    assert!(git_in(fx.repo_dir.path(), &["checkout", "-q", &fx.branch]));
    commit_notes(&fx, "main\n", "main edit");
    assert!(!git_in(fx.repo_dir.path(), &["cherry-pick", &side]));

    assert_refused(&fx, &logger, "git cherry-pick is in progress").await;
}

#[tokio::test]
async fn conflicted_revert_is_not_committed() {
    let (fx, logger) = setup_with_state();
    commit_notes(&fx, "one\n", "one");
    let first = fx.repo.rev_parse("HEAD").unwrap();
    commit_notes(&fx, "two\n", "two");
    assert!(!git_in(
        fx.repo_dir.path(),
        &["revert", "--no-edit", &first]
    ));

    assert_refused(&fx, &logger, "git revert is in progress").await;
}

#[tokio::test]
async fn a_merge_the_user_started_is_left_alone() {
    let (fx, logger) = setup_with_state();
    commit_notes(&fx, "base\n", "base");
    assert!(git_in(
        fx.repo_dir.path(),
        &["checkout", "-q", "-b", "drafts"]
    ));
    std::fs::write(fx.repo_dir.path().join("draft.md"), "draft\n").unwrap();
    assert!(git_in(fx.repo_dir.path(), &["add", "draft.md"]));
    assert!(git_in(fx.repo_dir.path(), &["commit", "-q", "-m", "draft"]));
    assert!(git_in(fx.repo_dir.path(), &["checkout", "-q", &fx.branch]));
    assert!(git_in(
        fx.repo_dir.path(),
        &["merge", "--no-commit", "--no-ff", "drafts"]
    ));
    assert!(fx.repo.merge_in_progress());

    assert_refused(&fx, &logger, "a merge you started is in progress").await;
    assert!(fx.repo.merge_in_progress());
}

#[tokio::test]
async fn a_conflicted_user_merge_is_not_kept_both_in_both_mode() {
    let (fx, logger) = setup_with_state();
    commit_notes(&fx, "base\n", "base");
    assert!(git_in(
        fx.repo_dir.path(),
        &["checkout", "-q", "-b", "drafts"]
    ));
    commit_notes(&fx, "draft version\n", "draft");
    assert!(git_in(fx.repo_dir.path(), &["checkout", "-q", &fx.branch]));
    commit_notes(&fx, "main version\n", "main");
    assert!(!git_in(fx.repo_dir.path(), &["merge", "drafts"]));

    assert_refused(&fx, &logger, "a merge you started is in progress").await;
    assert!(std::fs::read_to_string(fx.repo_dir.path().join("notes.md"))
        .unwrap()
        .contains("<<<<<<<"));
}

#[tokio::test]
async fn sync_never_pushes_local_state_when_its_ignore_rule_is_gone() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    // Drop every rule that ignores .CommitBook/local/, as a merge could.
    std::fs::write(root.join(".git/info/exclude"), "").unwrap();
    std::fs::write(
        root.join(".CommitBook/.gitignore"),
        "# emptied by a merge\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".CommitBook/local/auth.toml"),
        "[auth]\ntoken = \"ghp_secret\"\n",
    )
    .unwrap();
    std::fs::write(root.join("note.md"), "hello\n").unwrap();

    let outcome = sync_with_resolver(
        root,
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap();
    assert!(outcome.errors.is_empty(), "{outcome:?}");

    let remote = git2::Repository::open_bare(fx.remote_dir.path()).unwrap();
    let tree = remote
        .find_reference(&format!("refs/heads/{}", fx.branch))
        .unwrap()
        .peel_to_tree()
        .unwrap();
    assert!(tree.get_path(Path::new("note.md")).is_ok());
    assert!(tree.get_path(Path::new(".CommitBook/local")).is_err());
    // The ignore rule is restored and published for the other devices.
    let ignore = tree.get_path(Path::new(".CommitBook/.gitignore")).unwrap();
    let text = remote.find_blob(ignore.id()).unwrap().content().to_vec();
    assert!(String::from_utf8(text).unwrap().contains("/local/"));
}

#[tokio::test]
async fn sync_refuses_a_repository_with_a_git_crypt_filter() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    std::fs::write(root.join(".gitattributes"), "secret.md filter=git-crypt\n").unwrap();
    std::fs::write(root.join("secret.md"), "plaintext api key\n").unwrap();
    let head = fx.repo.rev_parse("HEAD").unwrap();
    let remote = remote_branch_oid(&fx);

    let error = sync_with_resolver(
        root,
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        &logger,
        None,
    )
    .await
    .unwrap_err();

    assert!(
        format!("{error:#}").contains("filter=git-crypt"),
        "{error:#}"
    );
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert_eq!(remote_branch_oid(&fx), remote);
}

#[tokio::test]
async fn both_mode_preserves_pending_rejected_and_stale_reviews() {
    for proposal_state in ["pending", "rejected", "stale"] {
        let (fx, logger) = setup_with_state();
        let root = fx.repo_dir.path();
        let mut config = LocalConfig::new("notes", &fx.branch, "origin");
        config.conflicts.mode = ConflictMode::Review;
        config.save(root).unwrap();
        diverge_files(&fx, &[("notes.md", "base\n", "local\n", "remote\n")]);
        let mut options = SyncOptions::new("origin", &fx.branch, true);
        options.review_ai_resolutions = true;
        // Review must win even if a caller enables both options, including
        // when the merge has just created the conflicts.
        options.keep_both = true;
        let resolver = MockResolver::ok("proposal\n");
        let first = sync_with_resolver(
            root,
            &options,
            Some(&resolver),
            &SystemCredentials,
            &logger,
            None,
        )
        .await
        .unwrap();
        assert_eq!(first.manual_conflicts, 1);
        assert!(first.kept_both.is_empty());
        assert_eq!(first.pushed, 0);
        let view = crate::review::list(root).unwrap().remove(0);
        assert!(view.proposal.is_some());
        match proposal_state {
            "rejected" => {
                crate::review::proposal_action(
                    root,
                    &crate::review::ResolutionInput {
                        path: view.path,
                        revision: view.revision,
                        proposal_version: view.proposal_version,
                        action: "reject".into(),
                        content: None,
                    },
                )
                .await
                .unwrap();
                assert!(
                    crate::review::list(root).unwrap()[0]
                        .proposal
                        .as_ref()
                        .unwrap()
                        .rejected
                );
            }
            "stale" => {
                std::fs::write(root.join("notes.md"), "user's in-progress resolution\n").unwrap();
                assert!(crate::review::list(root).unwrap()[0].proposal_stale);
            }
            _ => {}
        }
        config.conflicts.mode = ConflictMode::Both;
        config.save(root).unwrap();
        let before = std::fs::read(root.join("notes.md")).unwrap();
        let head = fx.repo.rev_parse("HEAD").unwrap();
        let remote = remote_branch_oid(&fx);
        let proposals_path = LocalConfig::local_dir(root).join("conflict-proposals.toml");
        let proposals = std::fs::read(&proposals_path).unwrap();
        let second = sync_repository(root, &config, &logger, None).await.unwrap();
        assert_eq!(second.manual_conflicts, 1, "{proposal_state}: {second:?}");
        assert!(!second.committed);
        assert_eq!(second.pushed, 0);
        assert!(second.kept_both.is_empty());
        assert_eq!(std::fs::read(root.join("notes.md")).unwrap(), before);
        assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
        assert_eq!(remote_branch_oid(&fx), remote);
        assert_eq!(std::fs::read(proposals_path).unwrap(), proposals);
        assert_eq!(fx.repo.list_conflicted_paths().unwrap(), ["notes.md"]);
        assert!(fx.repo.merge_in_progress());
        let state = SyncState::load(&cb_dir_of(&fx)).unwrap();
        assert_eq!(state.last_error_stage.as_deref(), Some("review"));
        assert!(state.last_error.unwrap().contains("await review"));
    }
}

/// Paths in the remote branch tip's tree.
fn remote_paths(fx: &RepoFixture) -> Vec<String> {
    let remote = git2::Repository::open_bare(fx.remote_dir.path()).unwrap();
    let tree = remote
        .find_reference(&format!("refs/heads/{}", fx.branch))
        .unwrap()
        .peel_to_tree()
        .unwrap();
    let mut paths = Vec::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            paths.push(format!("{dir}{}", entry.name().unwrap()));
        }
        git2::TreeWalkResult::Ok
    })
    .unwrap();
    paths
}

async fn sync_default(fx: &RepoFixture, logger: &FileLogger) -> Result<SyncOutcome> {
    sync_with_resolver(
        fx.repo_dir.path(),
        &SyncOptions::new("origin", &fx.branch, true),
        None,
        &SystemCredentials,
        logger,
        None,
    )
    .await
}

#[tokio::test]
async fn finder_ds_store_ignored_by_excludes_file_does_not_block_sync() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    set_repo_excludes(root, ".DS_Store\n");
    std::fs::write(root.join(".CommitBook/.DS_Store"), "\0\0\0\x01Bud1").unwrap();
    std::fs::write(root.join("draft.md"), "pending notes\n").unwrap();

    let outcome = sync_default(&fx, &logger).await.unwrap();
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert!(outcome.committed && outcome.pushed >= 1, "{outcome:?}");
    let remote = remote_paths(&fx);
    assert!(remote.contains(&"draft.md".to_string()), "{remote:?}");
    assert!(
        !remote.iter().any(|p| p.ends_with(".DS_Store")),
        "{remote:?}"
    );
    assert_eq!(
        std::fs::read(root.join(".CommitBook/.DS_Store")).unwrap(),
        b"\0\0\0\x01Bud1"
    );
    assert!(SyncState::load(&cb_dir_of(&fx))
        .unwrap()
        .last_error
        .is_none());
}

#[tokio::test]
async fn unignored_os_and_editor_files_in_metadata_are_never_published() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    set_repo_excludes(root, "");
    let mut junk = vec![
        ".DS_Store",
        "._config.toml",
        "Thumbs.db",
        ".config.toml.swp",
        "config.toml~",
        ".config.toml.abc123.tmp",
        "config 2.toml",
    ];
    if cfg!(unix) {
        junk.push("Icon\r");
    }
    for name in &junk {
        std::fs::write(root.join(".CommitBook").join(name), "junk").unwrap();
    }
    std::fs::write(root.join("draft.md"), "pending notes\n").unwrap();

    let outcome = sync_default(&fx, &logger).await.unwrap();
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    let remote = remote_paths(&fx);
    assert!(remote.contains(&"draft.md".to_string()), "{remote:?}");
    for name in &junk {
        let path = format!(".CommitBook/{name}");
        assert!(!remote.contains(&path), "{path:?} in {remote:?}");
        assert!(root.join(&path).exists(), "{path:?}");
    }

    // The junk alone is not a change: the next cycle commits nothing.
    let head = fx.repo.rev_parse("HEAD").unwrap();
    let outcome = sync_default(&fx, &logger).await.unwrap();
    assert!(!outcome.committed, "{outcome:?}");
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert!(SyncState::load(&cb_dir_of(&fx))
        .unwrap()
        .last_error
        .is_none());
}

#[tokio::test]
async fn legacy_metadata_is_never_published_and_does_not_block_sync() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    set_repo_excludes(root, "");
    let abandoned = root.join(".CommitBook/auth.toml");
    std::fs::write(&abandoned, "private fixture").unwrap();
    std::fs::create_dir(root.join(".CommitBook/logs")).unwrap();
    std::fs::write(root.join(".CommitBook/logs/launchd-stdout.log"), "old").unwrap();
    std::fs::write(root.join("draft.md"), "pending notes\n").unwrap();

    let outcome = sync_default(&fx, &logger).await.unwrap();
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    let remote = remote_paths(&fx);
    assert!(remote.contains(&"draft.md".to_string()), "{remote:?}");
    assert!(
        !remote
            .iter()
            .any(|p| p == ".CommitBook/auth.toml" || p.starts_with(".CommitBook/logs/")),
        "{remote:?}"
    );
    assert_eq!(
        std::fs::read_to_string(abandoned).unwrap(),
        "private fixture"
    );
}

#[tokio::test]
async fn metadata_file_committed_elsewhere_follows_git() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    set_repo_excludes(root, "");
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    std::fs::write(other.path().join(".CommitBook/.DS_Store"), "v1").unwrap();
    assert!(git_in(
        other.path(),
        &["add", "-f", ".CommitBook/.DS_Store"]
    ));
    assert!(git_in(other.path(), &["commit", "-q", "-m", "finder file"]));
    assert!(git_in(other.path(), &["push", "-q", "origin", &fx.branch]));

    sync_default(&fx, &logger).await.unwrap();
    let outcome = sync_default(&fx, &logger).await.unwrap();
    assert!(outcome.errors.is_empty(), "{outcome:?}");
    assert!(!outcome.committed, "{outcome:?}");

    // Once tracked, edits and deletions are published like any other file.
    std::fs::write(root.join(".CommitBook/.DS_Store"), "v2").unwrap();
    assert!(sync_default(&fx, &logger).await.unwrap().committed);
    std::fs::remove_file(root.join(".CommitBook/.DS_Store")).unwrap();
    assert!(sync_default(&fx, &logger).await.unwrap().committed);
    assert!(!remote_paths(&fx).contains(&".CommitBook/.DS_Store".to_string()));
}

#[tokio::test]
async fn sync_writes_only_known_top_level_metadata() {
    let (fx, logger) = setup_with_state();
    let root = fx.repo_dir.path();
    crate::commitbooks::init::init_dot_commitbook(
        root,
        "notes",
        &fx.branch,
        "origin",
        None,
        crate::config::Auth::Pat,
    )
    .unwrap();
    sync_default(&fx, &logger).await.unwrap();
    LocalConfig::load(root).unwrap().save(root).unwrap();

    for entry in std::fs::read_dir(root.join(".CommitBook")).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(
            matches!(
                name.to_str(),
                Some("config.toml" | ".gitignore" | "devices" | "local")
            ),
            "CommitBook wrote an unexpected top-level entry: {name:?}"
        );
    }
}
