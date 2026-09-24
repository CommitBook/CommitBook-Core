use super::*;

fn init_repo(path: &Path) -> git2::Repository {
    let mut options = git2::RepositoryInitOptions::new();
    options.initial_head("main");
    let repository = git2::Repository::init_opts(path, &options).unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    drop(config);
    repository
}

fn managed_clone() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let clone = root.path().join("owner__notes");
    std::fs::create_dir(&clone).unwrap();
    init_repo(&clone);
    git2::Repository::open(&clone)
        .unwrap()
        .remote("origin", "https://github.com/owner/notes.git")
        .unwrap();
    commitbook_engine::commitbooks::init_dot_commitbook(
        &clone,
        "Notes",
        "main",
        "origin",
        None,
        commitbook_engine::config::Auth::Pat,
    )
    .unwrap();
    std::fs::write(clone.join("base.md"), "base\n").unwrap();
    let repo = GitRepo::open(&clone).unwrap();
    repo.stage_all().unwrap();
    repo.commit("base").unwrap();
    (root, clone)
}

#[test]
fn walker_includes_nonignored_hidden_markdown_and_skips_gitignored_files() {
    let clone = tempfile::tempdir().unwrap();
    let repository = init_repo(clone.path());
    std::fs::create_dir_all(clone.path().join(".notes")).unwrap();
    std::fs::write(clone.path().join(".notes/visible.md"), "visible").unwrap();
    std::fs::write(clone.path().join("ignored.md"), "ignored").unwrap();
    std::fs::write(clone.path().join(".gitignore"), "ignored.md\n").unwrap();

    let mut files = Vec::new();
    walk_markdown(&repository, clone.path(), clone.path(), &mut files).unwrap();
    files.sort();
    assert_eq!(files, vec![".notes/visible.md"]);
}

#[cfg(unix)]
#[test]
fn walker_does_not_follow_symlink_directories_or_files() {
    use std::os::unix::fs::symlink;

    let clone = tempfile::tempdir().unwrap();
    let repository = init_repo(clone.path());
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "secret").unwrap();
    symlink(outside.path(), clone.path().join("linked-dir")).unwrap();
    symlink(
        outside.path().join("secret.md"),
        clone.path().join("linked.md"),
    )
    .unwrap();

    let mut files = Vec::new();
    walk_markdown(&repository, clone.path(), clone.path(), &mut files).unwrap();
    assert!(files.is_empty());
}

#[test]
fn save_commits_only_the_selected_document() {
    let (root, clone) = managed_clone();
    let repo = GitRepo::open(&clone).unwrap();
    std::fs::write(clone.join("unrelated.md"), "already staged\n").unwrap();
    repo.stage_paths(&["unrelated.md".to_string()]).unwrap();

    save_document(root.path(), "owner/notes", "daily/today.md", "today\n").unwrap();

    let repository = git2::Repository::open(&clone).unwrap();
    let tree = repository.head().unwrap().peel_to_tree().unwrap();
    assert!(tree.get_path(Path::new("daily/today.md")).is_ok());
    assert!(tree.get_path(Path::new("unrelated.md")).is_err());
    assert!(repository
        .index()
        .unwrap()
        .get_path(Path::new("unrelated.md"), 0)
        .is_some());
}

#[test]
fn save_reports_repository_lock_contention() {
    let (root, clone) = managed_clone();
    let _lock = RepoLock::acquire(&clone).unwrap();
    let error = save_document(root.path(), "owner/notes", "busy.md", "busy")
        .expect_err("second mutator must not enter the repository");
    assert!(matches!(error, CommitBookError::MergeError { .. }));
    assert!(!clone.join("busy.md").exists());
}

#[test]
fn save_refuses_to_commit_during_merge_resolution() {
    let (root, clone) = managed_clone();
    let repository = git2::Repository::open(&clone).unwrap();
    let head_before = repository.head().unwrap().target().unwrap();
    let index_before = std::fs::read(repository.path().join("index")).unwrap();
    let content_before = std::fs::read(clone.join("base.md")).unwrap();
    std::fs::write(
        repository.path().join("MERGE_HEAD"),
        format!("{head_before}\n"),
    )
    .unwrap();

    let error = save_document(root.path(), "owner/notes", "base.md", "changed\n")
        .expect_err("saving during a merge must fail");
    assert!(matches!(error, CommitBookError::MergeError { .. }));
    assert_eq!(repository.head().unwrap().target(), Some(head_before));
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index_before
    );
    assert_eq!(
        std::fs::read(clone.join("base.md")).unwrap(),
        content_before
    );
}

#[test]
fn save_refuses_wrong_branch_before_writing() {
    let (root, clone) = managed_clone();
    let repository = git2::Repository::open(&clone).unwrap();
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("other", &head, false).unwrap();
    drop(head);
    repository.set_head("refs/heads/other").unwrap();
    repository.checkout_head(None).unwrap();
    let before = std::fs::read(clone.join("base.md")).unwrap();

    let error = save_document(root.path(), "owner/notes", "base.md", "wrong branch\n")
        .expect_err("save must honor the configured branch");
    assert!(matches!(error, CommitBookError::InvalidInput { .. }));
    assert_eq!(std::fs::read(clone.join("base.md")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn document_api_never_writes_through_symlink_parent() {
    use std::os::unix::fs::symlink;

    let (root, clone) = managed_clone();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), clone.join("linked")).unwrap();

    assert!(save_document(root.path(), "owner/notes", "linked/secret.md", "secret").is_err());
    assert!(!outside.path().join("secret.md").exists());
}
