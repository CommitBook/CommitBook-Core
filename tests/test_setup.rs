use std::fs;
use std::process::Command;

/// Helper: create a temporary git repo in a temp directory.
fn create_temp_git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to init git repo");
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to set git email");
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to set git name");
    dir
}

/// Helper: get the path to the commitbook binary.
fn commitbook_bin() -> std::path::PathBuf {
    let mut path = std::env::current_exe()
        .expect("Failed to get current exe")
        .parent()
        .expect("Failed to get parent")
        .parent()
        .expect("Failed to get grandparent")
        .to_path_buf();
    path.push("commitbook");
    path
}

#[test]
fn test_setup_creates_commitbook_dir() {
    let repo = create_temp_git_repo();
    let cb_dir = repo.path().join(".CommitBook");
    let config_path = cb_dir.join("config.toml");
    let logs_dir = cb_dir.join("logs");

    // Run setup non-interactively by piping "1\n" for schedule choice
    let output = Command::new(commitbook_bin())
        .args(["setup", "--repo", &repo.path().to_string_lossy()])
        .env("RUST_LOG", "error")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(b"1\n");
            }
            child.wait_with_output()
        })
        .expect("Failed to run commitbook setup");

    assert!(output.status.success(), "Setup failed: {}", String::from_utf8_lossy(&output.stderr));
    assert!(cb_dir.exists(), ".CommitBook directory should exist");
    assert!(config_path.exists(), "config.toml should exist");
    assert!(logs_dir.exists(), "logs directory should exist");
}

#[test]
fn test_setup_creates_gitignore_entry() {
    let repo = create_temp_git_repo();

    let output = Command::new(commitbook_bin())
        .args(["setup", "--repo", &repo.path().to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(b"1\n");
            }
            child.wait_with_output()
        })
        .expect("Failed to run commitbook setup");

    assert!(output.status.success());

    let gitignore = repo.path().join(".gitignore");
    assert!(gitignore.exists(), ".gitignore should exist");
    let content = fs::read_to_string(&gitignore).expect("Failed to read .gitignore");
    assert!(
        content.contains(".CommitBook/logs/"),
        ".gitignore should contain .CommitBook/logs/"
    );
}

#[test]
fn test_setup_fails_on_non_git_dir() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(commitbook_bin())
        .args(["setup", "--repo", &dir.path().to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(b"1\n");
            }
            child.wait_with_output()
        })
        .expect("Failed to run commitbook");

    assert!(
        !output.status.success(),
        "Setup should fail on non-git directory"
    );
}

#[test]
fn test_setup_config_has_correct_defaults() {
    let repo = create_temp_git_repo();

    let output = Command::new(commitbook_bin())
        .args(["setup", "--repo", &repo.path().to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(b"1\n"); // Choose "hourly"
            }
            child.wait_with_output()
        })
        .expect("Failed to run commitbook setup");

    assert!(output.status.success());

    let config_path = repo.path().join(".CommitBook").join("config.toml");
    let content = fs::read_to_string(&config_path).expect("Failed to read config");

    assert!(content.contains("enabled = true"), "Should be enabled");
    assert!(
        content.contains(r#"schedule = "0 * * * *""#),
        "Default schedule should be hourly"
    );
    assert!(
        content.contains("auto_push = true"),
        "auto_push should default to true"
    );
}
