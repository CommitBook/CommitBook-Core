use super::*;
use crate::git::test_support::{clone_second_workdir, commit_and_push_from, setup_repo_with_bare_remote};
use crate::logger::FileLogger;

#[test]
fn test_migrate_removes_legacy_remote_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    let shadow = cb_dir.join("local").join("remote");
    std::fs::create_dir_all(&shadow).unwrap();
    std::fs::write(shadow.join("marker.txt"), "legacy").unwrap();

    let logger = FileLogger::new(tmp.path(), 30).unwrap();
    migrate_legacy_local_dirs(&cb_dir, &logger).unwrap();

    assert!(!shadow.exists(), "legacy shadow clone should be removed");
}

#[test]
fn test_migrate_removes_legacy_base_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    let base = cb_dir.join("local").join("base");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("notes.md"), "cached\n").unwrap();

    let logger = FileLogger::new(tmp.path(), 30).unwrap();
    migrate_legacy_local_dirs(&cb_dir, &logger).unwrap();

    assert!(!base.exists(), "legacy base dir should be removed");
}

#[test]
fn test_migrate_removes_both_legacy_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    let shadow = cb_dir.join("local").join("remote");
    let base = cb_dir.join("local").join("base");
    std::fs::create_dir_all(&shadow).unwrap();
    std::fs::create_dir_all(&base).unwrap();

    let logger = FileLogger::new(tmp.path(), 30).unwrap();
    migrate_legacy_local_dirs(&cb_dir, &logger).unwrap();

    assert!(!shadow.exists());
    assert!(!base.exists());
}

#[test]
fn test_migrate_noop_when_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local")).unwrap();

    let logger = FileLogger::new(tmp.path(), 30).unwrap();
    migrate_legacy_local_dirs(&cb_dir, &logger).unwrap();
}

#[test]
fn test_reconcile_skips_when_not_a_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let logger = FileLogger::new(tmp.path(), 30).unwrap();

    // Not a git repo — should be a silent no-op.
    reconcile_local_branch(tmp.path(), "main", &logger).unwrap();
}

#[test]
fn test_reconcile_fast_forwards_when_behind() {
    let fx = setup_repo_with_bare_remote();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote_only.md", "# remote\n");

    let logger = FileLogger::new(fx.repo_dir.path(), 30).unwrap();
    reconcile_local_branch(fx.repo_dir.path(), &fx.branch, &logger).unwrap();

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
    let err = reconcile_local_branch(fx.repo_dir.path(), &fx.branch, &logger)
        .expect_err("expected divergence error");
    let msg = format!("{err:#}");
    assert!(msg.contains("diverged"), "unexpected error: {msg}");
}
