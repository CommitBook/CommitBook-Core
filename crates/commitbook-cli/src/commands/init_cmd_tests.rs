use super::*;
use git2::{Repository, RepositoryInitOptions, Signature, Status};
use std::path::Path;
use tempfile::tempdir;

fn initialize_repo_with_remote_on_branch(branch: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let remote = tempdir().unwrap();
    Repository::init_bare(remote.path()).unwrap();

    let local = tempdir().unwrap();
    let mut options = RepositoryInitOptions::new();
    options.initial_head(branch);
    let repo = Repository::init_opts(local.path(), &options).unwrap();
    {
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "CommitBook Test").unwrap();
        config
            .set_str("user.email", "test@commitbook.local")
            .unwrap();
        config.set_bool("commit.gpgsign", false).unwrap();
    }
    std::fs::write(local.path().join("README.md"), "# Notes\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("README.md")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let signature = Signature::now("CommitBook Test", "test@commitbook.local").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
        .unwrap();
    repo.remote("origin", &format!("file://{}", remote.path().display()))
        .unwrap();
    drop(tree);
    drop(repo);

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
    let repo = Repository::open(local.path()).unwrap();
    std::fs::write(local.path().join("unrelated.txt"), "keep staged\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("unrelated.txt")).unwrap();
    index.write().unwrap();
    drop(index);
    drop(repo);

    initialize_and_publish(local.path(), Some("origin")).unwrap();

    let repo = Repository::open(local.path()).unwrap();
    assert!(repo
        .status_file(Path::new("unrelated.txt"))
        .unwrap()
        .contains(Status::INDEX_NEW));
    let head_tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(head_tree.get_path(Path::new("unrelated.txt")).is_err());
    assert!(head_tree
        .get_path(Path::new(".CommitBook/config.toml"))
        .is_ok());
    assert!(head_tree
        .get_path(Path::new(".CommitBook/.gitignore"))
        .is_ok());
    assert!(!local.path().join(".gitignore").exists());

    let remote_repo = Repository::open_bare(remote.path()).unwrap();
    let remote_tree = remote_repo
        .find_reference("refs/heads/main")
        .unwrap()
        .peel_to_tree()
        .unwrap();
    assert!(remote_tree
        .get_path(Path::new(".CommitBook/config.toml"))
        .is_ok());
    assert!(remote_tree
        .get_path(Path::new(".CommitBook/.gitignore"))
        .is_ok());
}

#[test]
fn failed_initialization_push_leaves_metadata_committed() {
    let (local, _remote) = initialize_repo_with_remote();
    let repo = Repository::open(local.path()).unwrap();
    repo.remote_set_url("origin", "file:///definitely/missing/commitbook.git")
        .unwrap();
    drop(repo);

    let error = initialize_and_publish(local.path(), Some("origin")).unwrap_err();
    assert!(error.to_string().contains("committed locally"));

    let repo = Repository::open(local.path()).unwrap();
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(tree.get_path(Path::new(".CommitBook/config.toml")).is_ok());
    assert!(tree.get_path(Path::new(".CommitBook/.gitignore")).is_ok());
}

#[test]
fn new_initialization_uses_the_checked_out_branch() {
    let (local, remote) = initialize_repo_with_remote_on_branch("notes");

    initialize_and_publish(local.path(), Some("origin")).unwrap();

    let config = LocalConfig::load(local.path()).unwrap();
    assert_eq!(config.git.branch, "notes");
    let remote_repo = Repository::open_bare(remote.path()).unwrap();
    let remote_tree = remote_repo
        .find_reference("refs/heads/notes")
        .unwrap()
        .peel_to_tree()
        .unwrap();
    assert!(remote_tree
        .get_path(Path::new(".CommitBook/config.toml"))
        .is_ok());
}

#[test]
fn initialization_refuses_to_commit_metadata_on_another_branch() {
    let (local, remote) = initialize_repo_with_remote();
    state::initialize(local.path(), "origin").unwrap();
    let repo = Repository::open(local.path()).unwrap();
    let main = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("other", &main, false).unwrap();
    repo.set_head("refs/heads/other").unwrap();
    repo.checkout_head(None).unwrap();
    drop(main);
    drop(repo);

    let error = initialize_and_publish(local.path(), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not match configured branch"));

    let repo = Repository::open(local.path()).unwrap();
    assert_eq!(repo.head().unwrap().shorthand(), Some("other"));
    let remote_repo = Repository::open_bare(remote.path()).unwrap();
    let remote_tree = remote_repo
        .find_reference("refs/heads/main")
        .unwrap()
        .peel_to_tree()
        .unwrap();
    assert!(remote_tree
        .get_path(Path::new(".CommitBook/config.toml"))
        .is_err());
}
