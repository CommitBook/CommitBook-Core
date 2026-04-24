use super::*;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_find_commitbook_dir_finds_in_git_repo() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join(".CommitBook")).unwrap();

    let found = find_commitbook_dir_from(repo).unwrap();
    assert_eq!(found, repo.join(".CommitBook"));
}

#[test]
fn test_find_commitbook_dir_finds_from_subdir() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let subdir = repo.join("notes").join("sub");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join(".CommitBook")).unwrap();

    let found = find_commitbook_dir_from(&subdir).unwrap();
    assert_eq!(found, repo.join(".CommitBook"));
}

#[test]
fn test_find_commitbook_dir_skips_commitbook_without_git() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".CommitBook")).unwrap();
    // No .git — must not return this directory

    let err = find_commitbook_dir_from(repo).unwrap_err().to_string();
    assert!(err.contains("not initialized"));
}

#[test]
fn test_find_commitbook_dir_errors_when_not_found() {
    let tmp = tempdir().unwrap();
    let err = find_commitbook_dir_from(tmp.path()).unwrap_err().to_string();
    assert!(err.contains("not initialized"));
}

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

    initialize(repo, "origin").unwrap();

    assert!(repo.join(".CommitBook").is_dir());
    assert!(repo.join(".CommitBook/local").is_dir());
    assert!(repo.join(".CommitBook/local/logs").is_dir());
    assert!(repo.join(".CommitBook/config.toml").exists());
}

#[test]
fn test_initialize_updates_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo, "origin").unwrap();

    let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gitignore.contains(".CommitBook/local/"));
}

#[test]
fn test_initialize_persists_remote_name() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo, "upstream").unwrap();

    let config = crate::config::LocalConfig::load(repo).unwrap();
    assert_eq!(config.git.remote, "upstream");
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

    initialize(repo, "origin").unwrap();

    // Config should not be overwritten.
    let content = std::fs::read_to_string(cb_dir.join("config.toml")).unwrap();
    assert!(content.contains("custom"));
}

#[test]
fn test_initialize_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo, "origin").unwrap();
    initialize(repo, "origin").unwrap(); // Should not error.

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
    let count = content.matches(".CommitBook/local/").count();
    assert_eq!(count, 1);
}

#[test]
fn test_ensure_initialized_does_not_auto_init() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    // A git repo without .CommitBook/ must NOT get auto-initialized.
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(repo).unwrap();
    let result = ensure_initialized();
    std::env::set_current_dir(original).unwrap();

    assert!(result.is_err());
    assert!(!repo.join(".CommitBook").exists());
}
