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

    initialize_and_publish(local.path(), Some("origin"), Some("Laptop")).unwrap();

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

    let error = initialize_and_publish(local.path(), Some("origin"), Some("Laptop")).unwrap_err();
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

    initialize_and_publish(local.path(), Some("origin"), Some("Laptop")).unwrap();

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
    state::initialize(local.path(), "origin", "main").unwrap();
    git(local.path(), &["branch", "other"]);
    git(local.path(), &["checkout", "other"]);

    let error = initialize_and_publish(local.path(), None, None)
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
fn initialization_registers_and_publishes_this_device() {
    let (local, remote) = initialize_repo_with_remote();

    initialize_and_publish(local.path(), Some("origin"), Some("Laptop")).unwrap();

    let (id, device) = devices::this_device(local.path()).unwrap().unwrap();
    assert_eq!(device.name, "Laptop");
    // A `file://` remote is not SSH, so desktop auth is the local repo mode.
    assert_eq!(
        device.auth,
        commitbook_engine::config::Auth::ExistingLocalRepo
    );
    let device_file = devices::device_repo_path(&id);
    assert!(tree_contains(local.path(), "HEAD", &device_file));
    assert!(tree_contains(
        remote.path(),
        "refs/heads/main",
        &device_file
    ));
    assert!(!tree_contains(
        local.path(),
        "HEAD",
        ".CommitBook/local/device-id"
    ));
    // The CommitBook is named after the repository in the remote URL.
    let config = LocalConfig::load(local.path()).unwrap();
    assert_eq!(
        config.commitbook.name,
        remote.path().file_name().unwrap().to_str().unwrap()
    );
}

#[test]
fn joining_device_registers_on_an_initialized_clone() {
    let (local, remote) = initialize_repo_with_remote();
    initialize_and_publish(local.path(), Some("origin"), Some("Laptop")).unwrap();

    let other = tempdir().unwrap();
    let remote_url = format!("file://{}", remote.path().display());
    git(
        other.path(),
        &[
            "clone",
            "-q",
            "-b",
            "main",
            &remote_url,
            other.path().to_str().unwrap(),
        ],
    );
    git(other.path(), &["config", "user.name", "CommitBook Test"]);
    git(
        other.path(),
        &["config", "user.email", "test@commitbook.local"],
    );
    git(other.path(), &["config", "commit.gpgsign", "false"]);

    initialize_and_publish(other.path(), None, Some("Phone")).unwrap();

    let (devices, _) = devices::list(other.path()).unwrap();
    let names: Vec<_> = devices.iter().map(|d| d.device.name.as_str()).collect();
    assert_eq!(names, ["Laptop", "Phone"]);
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
