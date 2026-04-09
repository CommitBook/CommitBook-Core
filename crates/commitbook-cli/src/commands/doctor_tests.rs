use super::*;
use std::process::Command as ProcessCommand;

fn git_init(path: &Path) {
    ProcessCommand::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .output()
        .expect("git init failed");
}

#[test]
fn test_check_struct_pass() {
    let c = Check::pass("Test", "detail");
    assert!(c.passed);
    assert!(c.required);
    assert_eq!(c.name, "Test");
    assert_eq!(c.detail, "detail");
}

#[test]
fn test_check_struct_optional_fail() {
    let c = Check::optional_fail("Test", "detail");
    assert!(!c.passed);
    assert!(!c.required);
}

#[test]
fn test_check_git_repo_valid() {
    let tmp = tempfile::tempdir().unwrap();
    git_init(tmp.path());
    let c = check_git_repo(tmp.path());
    assert!(c.passed);
}

#[test]
fn test_check_git_repo_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    let c = check_git_repo(tmp.path());
    assert!(!c.passed);
}

#[test]
fn test_check_commitbook_config_not_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let c = check_commitbook_config(tmp.path());
    assert!(!c.passed);
    assert!(c.detail.contains("Not set up"));
}

#[test]
fn test_check_commitbook_config_valid() {
    let tmp = tempfile::tempdir().unwrap();
    LocalConfig::init(tmp.path(), "0 * * * *").unwrap();
    let c = check_commitbook_config(tmp.path());
    assert!(c.passed);
    assert!(c.detail.contains("enabled"));
}

#[test]
fn test_check_commitbook_config_corrupt() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(&cb_dir).unwrap();
    std::fs::write(cb_dir.join("config.toml"), "not valid toml {{{{").unwrap();
    let c = check_commitbook_config(tmp.path());
    assert!(!c.passed);
    assert!(c.detail.contains("Corrupt"));
}

#[test]
fn test_check_logs_writable_no_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let c = check_logs_writable(tmp.path());
    assert!(!c.passed);
    assert!(!c.required);
}

#[test]
fn test_check_logs_writable_creates_and_writes() {
    let tmp = tempfile::tempdir().unwrap();
    LocalConfig::init(tmp.path(), "0 * * * *").unwrap();
    // Remove the logs dir that init created, so check_logs_writable has to recreate it
    let logs_dir = tmp.path().join(".CommitBook").join("logs");
    if logs_dir.exists() {
        std::fs::remove_dir_all(&logs_dir).unwrap();
    }
    let c = check_logs_writable(tmp.path());
    assert!(c.passed);
    assert!(logs_dir.exists());
}
