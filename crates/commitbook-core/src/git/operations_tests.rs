use super::*;
use std::fs;

fn create_temp_repo() -> (tempfile::TempDir, Repository) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    // Set user config so commits work
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test User").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();

    (tmp, repo)
}

// --- ChangesSummary tests ---

#[test]
fn test_changes_summary_empty() {
    let s = ChangesSummary::default();
    assert!(s.is_empty());
    assert_eq!(s.total(), 0);
}

#[test]
fn test_changes_summary_total() {
    let s = ChangesSummary {
        new_files: vec!["a.rs".into()],
        modified_files: vec!["b.rs".into(), "c.rs".into()],
        deleted_files: vec![],
    };
    assert_eq!(s.total(), 3);
    assert!(!s.is_empty());
}

#[test]
fn test_changes_summary_text_empty() {
    let s = ChangesSummary::default();
    assert_eq!(s.to_summary_text(), "no changes");
}

#[test]
fn test_changes_summary_text_mixed() {
    let s = ChangesSummary {
        new_files: vec!["a.rs".into()],
        modified_files: vec!["b.rs".into()],
        deleted_files: vec!["c.rs".into()],
    };
    let text = s.to_summary_text();
    assert!(text.contains("1 new"));
    assert!(text.contains("1 modified"));
    assert!(text.contains("1 deleted"));
}

#[test]
fn test_changes_detail_text() {
    let s = ChangesSummary {
        new_files: vec!["main.rs".into()],
        modified_files: vec![],
        deleted_files: vec![],
    };
    let detail = s.to_detail_text();
    assert!(detail.contains("main.rs"));
    assert!(detail.contains("1 new file(s)"));
}

#[test]
fn test_changes_detail_text_empty() {
    let s = ChangesSummary::default();
    assert_eq!(s.to_detail_text(), "No changes");
}

// --- GitRepo tests ---

#[test]
fn test_is_repo_true() {
    let (tmp, _repo) = create_temp_repo();
    assert!(GitRepo::is_repo(tmp.path()));
}

#[test]
fn test_is_repo_false() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!GitRepo::is_repo(tmp.path()));
}

#[test]
fn test_open_repo() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path());
    assert!(git_repo.is_ok());
}

#[test]
fn test_has_remote_false() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();
    assert!(!git_repo.has_remote());
}

#[test]
fn test_current_branch_initial() {
    let (tmp, _repo) = create_temp_repo();
    // Need at least one commit for HEAD to exist
    let git_repo = GitRepo::open(tmp.path()).unwrap();
    fs::write(tmp.path().join("init.txt"), "hello").unwrap();
    git_repo.stage_all().unwrap();
    git_repo.commit("initial").unwrap();

    let branch = git_repo.current_branch().unwrap();
    // git init creates "main" or "master" depending on config
    assert!(!branch.is_empty());
}

#[test]
fn test_stage_and_commit_lifecycle() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();

    // Create a file
    fs::write(tmp.path().join("test.txt"), "content").unwrap();

    // Check changes
    let changes = git_repo.changes_summary().unwrap();
    assert!(!changes.is_empty());
    assert_eq!(changes.new_files.len(), 1);

    // Stage all
    git_repo.stage_all().unwrap();

    // Commit
    let hash = git_repo.commit("test commit").unwrap();
    assert_eq!(hash.len(), 7);

    // After commit, no changes
    let changes = git_repo.changes_summary().unwrap();
    assert!(changes.is_empty());
}

#[test]
fn test_changes_summary_modified() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();

    // Initial commit
    let file_path = tmp.path().join("file.txt");
    fs::write(&file_path, "v1").unwrap();
    git_repo.stage_all().unwrap();
    git_repo.commit("init").unwrap();

    // Modify file
    fs::write(&file_path, "v2").unwrap();
    let changes = git_repo.changes_summary().unwrap();
    assert_eq!(changes.modified_files.len(), 1);
}

#[test]
fn test_changes_summary_deleted() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();

    // Initial commit
    let file_path = tmp.path().join("doomed.txt");
    fs::write(&file_path, "bye").unwrap();
    git_repo.stage_all().unwrap();
    git_repo.commit("init").unwrap();

    // Delete file
    fs::remove_file(&file_path).unwrap();
    let changes = git_repo.changes_summary().unwrap();
    assert_eq!(changes.deleted_files.len(), 1);
}
