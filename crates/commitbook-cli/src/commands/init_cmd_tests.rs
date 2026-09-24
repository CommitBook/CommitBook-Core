use super::*;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn git_output(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap()
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = git_output(repo, args);
    assert!(
        output.status.success(),
        "git -C {} {args:?} failed\nstdout: {}\nstderr: {}",
        repo.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn tree_contains(repo: &Path, reference: &str, path: &str) -> bool {
    let object = format!("{reference}:{path}");
    git_output(repo, &["cat-file", "-e", &object])
        .status
        .success()
}

fn initialize_repo_with_remote_on_branch(branch: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let remote = tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]);

    let local = tempdir().unwrap();
    git(local.path(), &["init"]);
    let branch_ref = format!("refs/heads/{branch}");
    git(local.path(), &["symbolic-ref", "HEAD", &branch_ref]);
    git(local.path(), &["config", "user.name", "CommitBook Test"]);
    git(
        local.path(),
        &["config", "user.email", "test@commitbook.local"],
    );
    git(local.path(), &["config", "commit.gpgsign", "false"]);
    git(local.path(), &["config", "core.hooksPath", "/dev/null"]);
    std::fs::write(local.path().join("README.md"), "# Notes\n").unwrap();
    git(local.path(), &["add", "README.md"]);
    git(local.path(), &["commit", "-m", "base"]);
    let remote_url = format!("file://{}", remote.path().display());
    git(local.path(), &["remote", "add", "origin", &remote_url]);

    let repo = GitRepo::open(local.path()).unwrap();
    repo.push("origin", branch).unwrap();
    (local, remote)
}

fn initialize_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
    initialize_repo_with_remote_on_branch("main")
}

#[test]
fn initialization_commits_only_metadata_and_pushes_it() {
    let (local, remote) = initialize_repo_with_remote();
    std::fs::write(local.path().join("unrelated.txt"), "keep staged\n").unwrap();
    git(local.path(), &["add", "unrelated.txt"]);

    initialize_and_publish(local.path(), Some("origin")).unwrap();

    let staged = git(
        local.path(),
        &["diff", "--cached", "--name-only", "--", "unrelated.txt"],
    );
    assert_eq!(staged.trim(), "unrelated.txt");
    assert!(!tree_contains(local.path(), "HEAD", "unrelated.txt"));
    assert!(tree_contains(
        local.path(),
        "HEAD",
        ".CommitBook/config.toml"
    ));
    assert!(tree_contains(
        local.path(),
        "HEAD",
        ".CommitBook/.gitignore"
    ));
    assert!(!local.path().join(".gitignore").exists());

    assert!(tree_contains(
        remote.path(),
        "refs/heads/main",
        ".CommitBook/config.toml"
    ));
    assert!(tree_contains(
        remote.path(),
        "refs/heads/main",
        ".CommitBook/.gitignore"
    ));
}

#[test]
fn failed_initialization_push_leaves_metadata_committed() {
    let (local, _remote) = initialize_repo_with_remote();
    git(
        local.path(),
        &[
            "remote",
            "set-url",
            "origin",
            "file:///definitely/missing/commitbook.git",
        ],
    );

    let error = initialize_and_publish(local.path(), Some("origin")).unwrap_err();
    assert!(error.to_string().contains("committed locally"));

    assert!(tree_contains(
        local.path(),
        "HEAD",
        ".CommitBook/config.toml"
    ));
    assert!(tree_contains(
        local.path(),
        "HEAD",
        ".CommitBook/.gitignore"
    ));
}

#[test]
fn new_initialization_uses_the_checked_out_branch() {
    let (local, remote) = initialize_repo_with_remote_on_branch("notes");

    initialize_and_publish(local.path(), Some("origin")).unwrap();

    let config = LocalConfig::load(local.path()).unwrap();
    assert_eq!(config.git.branch, "notes");
    assert!(tree_contains(
        remote.path(),
        "refs/heads/notes",
        ".CommitBook/config.toml"
    ));
}

#[test]
fn initialization_refuses_to_commit_metadata_on_another_branch() {
    let (local, remote) = initialize_repo_with_remote();
    state::initialize(local.path(), "origin").unwrap();
    git(local.path(), &["branch", "other"]);
    git(local.path(), &["checkout", "other"]);

    let error = initialize_and_publish(local.path(), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not match configured branch"));

    assert_eq!(
        git(local.path(), &["branch", "--show-current"]).trim(),
        "other"
    );
    assert!(!tree_contains(
        remote.path(),
        "refs/heads/main",
        ".CommitBook/config.toml"
    ));
}

#[test]
fn init_with_auto_push_disabled_records_pending_without_pushing() {
    let (local, remote) = initialize_repo_with_remote();
    state::initialize(local.path(), "origin").unwrap();
    let mut config = LocalConfig::load(local.path()).unwrap();
    config.git.auto_push = false;
    config.save(local.path()).unwrap();
    // Publication must not depend on the remote being reachable.
    git(
        local.path(),
        &[
            "remote",
            "set-url",
            "origin",
            "file:///definitely/missing/commitbook.git",
        ],
    );

    initialize_and_publish(local.path(), None).unwrap();

    let cb_dir = LocalConfig::commitbook_dir(local.path());
    let pending = commitbook_engine::state::sync_state::SyncState::load(&cb_dir)
        .unwrap()
        .pending_init_push
        .expect("pending publication recorded");
    let head = git(local.path(), &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(pending.commit_oid, head);
    assert_eq!(pending.remote, "origin");
    assert_eq!(pending.branch, "main");
    assert!(tree_contains(
        local.path(),
        "HEAD",
        ".CommitBook/config.toml"
    ));
    assert!(!tree_contains(
        remote.path(),
        "refs/heads/main",
        ".CommitBook/config.toml"
    ));

    // Re-enabling auto_push publishes the recorded commit without a new one.
    let remote_url = format!("file://{}", remote.path().display());
    git(local.path(), &["remote", "set-url", "origin", &remote_url]);
    let mut config = LocalConfig::load(local.path()).unwrap();
    config.git.auto_push = true;
    config.save(local.path()).unwrap();
    initialize_and_publish(local.path(), None).unwrap();

    assert!(
        commitbook_engine::state::sync_state::SyncState::load(&cb_dir)
            .unwrap()
            .pending_init_push
            .is_none()
    );
    assert!(tree_contains(
        remote.path(),
        "refs/heads/main",
        ".CommitBook/config.toml"
    ));
}

#[test]
fn confirmation_accepts_enter_and_yes() {
    for answer in ["", "\n", "y", "Y", "yes", " Yes \n"] {
        assert!(accepts(answer), "{answer:?} should accept");
    }
}

#[test]
fn confirmation_declines_anything_else() {
    for answer in ["n", "N\n", "no", "nope", "x"] {
        assert!(!accepts(answer), "{answer:?} should decline");
    }
}
