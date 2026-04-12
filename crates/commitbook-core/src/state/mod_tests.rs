use super::*;
use std::path::PathBuf;

#[test]
fn test_repo_root_from_commitbook_dir() {
    let cb_dir = PathBuf::from("/home/user/notes/.CommitBook");
    assert_eq!(repo_root(&cb_dir), PathBuf::from("/home/user/notes"));
}

#[test]
fn test_initialize_creates_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    // Create a fake .git so initialize finds a git repo.
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo).unwrap();

    assert!(repo.join(".CommitBook").is_dir());
    assert!(repo.join(".CommitBook/base").is_dir());
    assert!(repo.join(".CommitBook/logs").is_dir());
    assert!(repo.join(".CommitBook/config.toml").exists());
}

#[test]
fn test_initialize_updates_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo).unwrap();

    let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gitignore.contains(".CommitBook/auth.toml"));
    assert!(gitignore.contains(".CommitBook/state.toml"));
    assert!(gitignore.contains(".CommitBook/base/"));
    assert!(gitignore.contains(".CommitBook/logs/"));
    assert!(gitignore.contains(".CommitBook/.lock"));
}

#[test]
fn test_initialize_does_not_overwrite_existing_config() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    // Write a config manually.
    let cb_dir = repo.join(".CommitBook");
    std::fs::create_dir_all(&cb_dir).unwrap();
    std::fs::write(
        cb_dir.join("config.toml"),
        "config_version = \"1\"\nenabled = true\nschedule = \"custom\"\ncreated_at = \"2026-01-01T00:00:00Z\"\n",
    )
    .unwrap();

    initialize(repo).unwrap();

    // Config should not be overwritten.
    let content = std::fs::read_to_string(cb_dir.join("config.toml")).unwrap();
    assert!(content.contains("custom"));
}

#[test]
fn test_initialize_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo).unwrap();
    initialize(repo).unwrap(); // Should not error.

    assert!(repo.join(".CommitBook/config.toml").exists());
}

#[test]
fn test_gitignore_does_not_duplicate_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    // Call update_gitignore twice.
    update_gitignore(repo).unwrap();
    update_gitignore(repo).unwrap();

    let content = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
    let count = content.matches(".CommitBook/auth.toml").count();
    assert_eq!(count, 1);
}
