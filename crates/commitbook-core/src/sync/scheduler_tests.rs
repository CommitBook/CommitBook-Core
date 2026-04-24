use super::*;
use crate::git::test_support::{clone_second_workdir, commit_and_push_from, setup_repo_with_bare_remote};
use crate::logger::FileLogger;

#[test]
fn test_reconcile_skips_when_not_a_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let logger = FileLogger::new(tmp.path(), 30).unwrap();

    // Not a git repo — should be a silent no-op.
    reconcile_local_branch(tmp.path(), "origin", "main", &logger).unwrap();
}

#[test]
fn test_reconcile_fast_forwards_when_behind() {
    let fx = setup_repo_with_bare_remote();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote_only.md", "# remote\n");

    let logger = FileLogger::new(fx.repo_dir.path(), 30).unwrap();
    reconcile_local_branch(fx.repo_dir.path(), "origin", &fx.branch, &logger).unwrap();

    let (ahead, behind) = fx
        .repo
        .ahead_behind("HEAD", &format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!((ahead, behind), (0, 0));
}

#[test]
fn test_reconcile_pushes_when_ahead() {
    let fx = setup_repo_with_bare_remote();

    // Local commits a non-markdown file (the kind the pipeline's markdown-only
    // push would normally skip).
    std::fs::write(fx.repo_dir.path().join("notes.txt"), "# local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add notes.txt").unwrap();
    let local_sha = fx.repo.rev_parse("HEAD").unwrap();

    // Before reconcile: remote still at the initial commit.
    let before_remote = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
    assert_ne!(local_sha, before_remote);

    let logger = FileLogger::new(fx.repo_dir.path(), 30).unwrap();
    reconcile_local_branch(fx.repo_dir.path(), "origin", &fx.branch, &logger).unwrap();

    // Reconcile fetched and pushed — remote now matches local HEAD.
    fx.repo.fetch("origin", &fx.branch).unwrap();
    let after_remote = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!(after_remote, local_sha);

    let (ahead, behind) = fx
        .repo
        .ahead_behind("HEAD", &format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!((ahead, behind), (0, 0));
}

#[test]
fn test_reconcile_errors_on_true_divergence() {
    let fx = setup_repo_with_bare_remote();

    // Local commit only.
    std::fs::write(fx.repo_dir.path().join("local.md"), "local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local only").unwrap();

    // Remote commit on a different path.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote_only.md", "# remote\n");

    let logger = FileLogger::new(fx.repo_dir.path(), 30).unwrap();
    let err = reconcile_local_branch(fx.repo_dir.path(), "origin", &fx.branch, &logger)
        .expect_err("expected divergence error");
    let msg = format!("{err:#}");
    assert!(msg.contains("diverged"), "unexpected error: {msg}");
}
