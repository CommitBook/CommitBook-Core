use std::fs;
use std::process::Command;

/// Helper: create a temporary git repo with initial commit.
fn create_temp_git_repo_with_commit() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let path = dir.path();

    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("Failed to init git repo");
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("Failed to set git email");
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .output()
        .expect("Failed to set git name");

    // Create initial file and commit
    fs::write(path.join("README.md"), "# Test\n").expect("Failed to write README");
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .expect("Failed to git add");
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(path)
        .output()
        .expect("Failed to initial commit");

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

/// Helper: run setup on a repo (non-interactive, hourly) and commit the setup files.
fn setup_repo(repo_path: &std::path::Path) {
    let output = Command::new(commitbook_bin())
        .args(["setup", "--repo", &repo_path.to_string_lossy()])
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
    assert!(output.status.success(), "Setup failed");

    // Commit the setup files so they don't show as changes
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo_path)
        .output()
        .expect("Failed to git add setup files");
    Command::new("git")
        .args(["commit", "-m", "CommitBook setup"])
        .current_dir(repo_path)
        .output()
        .expect("Failed to commit setup files");
}

#[test]
fn test_auto_commit_with_changes() {
    let repo = create_temp_git_repo_with_commit();
    let path = repo.path();
    setup_repo(path);

    // Create a new file (simulating note-taking)
    fs::write(path.join("notes.md"), "# My Notes\nSome content\n")
        .expect("Failed to write notes");

    // Run auto-commit
    let output = Command::new(commitbook_bin())
        .args(["auto-commit", "--repo", &path.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run auto-commit");

    assert!(
        output.status.success(),
        "Auto-commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the commit was made
    let log_output = Command::new("git")
        .args(["log", "--oneline", "-n", "1"])
        .current_dir(path)
        .output()
        .expect("Failed to get git log");

    let last_commit = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        last_commit.contains("Writing"),
        "Commit message should contain 'Writing' (fallback): got '{}'",
        last_commit.trim()
    );
}

#[test]
fn test_auto_commit_no_changes() {
    let repo = create_temp_git_repo_with_commit();
    let path = repo.path();
    setup_repo(path);

    // Count commits before auto-commit
    let log_before = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(path)
        .output()
        .expect("Failed to count commits");
    let count_before: usize = String::from_utf8_lossy(&log_before.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    // Run auto-commit without making any new changes
    let output = Command::new(commitbook_bin())
        .args(["auto-commit", "--repo", &path.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run auto-commit");

    assert!(output.status.success(), "Auto-commit should succeed even with no changes");

    // Count commits after auto-commit
    let log_after = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(path)
        .output()
        .expect("Failed to count commits");
    let count_after: usize = String::from_utf8_lossy(&log_after.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    assert_eq!(
        count_before, count_after,
        "No new commits should be created when there are no changes (before: {}, after: {})",
        count_before, count_after
    );
}

#[test]
fn test_auto_commit_creates_log() {
    let repo = create_temp_git_repo_with_commit();
    let path = repo.path();
    setup_repo(path);

    // Create a change
    fs::write(path.join("journal.md"), "# Journal\nEntry 1\n")
        .expect("Failed to write journal");

    // Run auto-commit
    let output = Command::new(commitbook_bin())
        .args(["auto-commit", "--repo", &path.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run auto-commit");

    assert!(output.status.success());

    // Check that a log file was created
    let logs_dir = path.join(".CommitBook").join("logs");
    let log_files: Vec<_> = fs::read_dir(&logs_dir)
        .expect("Failed to read logs dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "log")
                .unwrap_or(false)
        })
        .collect();

    assert!(!log_files.is_empty(), "Should have at least one log file");

    // Verify log content
    let log_content = fs::read_to_string(log_files[0].path())
        .expect("Failed to read log file");
    assert!(
        log_content.contains("Auto-commit cycle started"),
        "Log should contain cycle start"
    );
    assert!(
        log_content.contains("completed successfully"),
        "Log should contain success message"
    );
}

#[test]
fn test_auto_commit_without_setup_fails() {
    let repo = create_temp_git_repo_with_commit();
    let path = repo.path();

    // Run auto-commit WITHOUT running setup first
    let output = Command::new(commitbook_bin())
        .args(["auto-commit", "--repo", &path.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run auto-commit");

    assert!(
        !output.status.success(),
        "Auto-commit should fail without setup"
    );
}
