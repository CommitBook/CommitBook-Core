use std::process::Command;

/// Helper: create a temporary git repo.
fn create_temp_git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to init git repo");
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
fn test_doctor_runs_without_crash() {
    let repo = create_temp_git_repo();

    let output = Command::new(commitbook_bin())
        .args(["doctor", "--repo", &repo.path().to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run commitbook doctor");

    // Doctor should always succeed (it reports status, not errors)
    assert!(
        output.status.success(),
        "Doctor should not crash: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Git"),
        "Output should contain Git check"
    );
}

#[test]
fn test_doctor_detects_missing_config() {
    let repo = create_temp_git_repo();

    let output = Command::new(commitbook_bin())
        .args(["doctor", "--repo", &repo.path().to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run commitbook doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show that config is not set up
    assert!(
        stdout.contains("Not set up") || stdout.contains("commitbook setup"),
        "Should report missing config"
    );
}

#[test]
fn test_doctor_on_non_git_dir() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(commitbook_bin())
        .args(["doctor", "--repo", &dir.path().to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run commitbook doctor");

    // Doctor should still succeed but report the issue
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Not a git repo") || stdout.contains("failed"),
        "Should report non-git directory"
    );
}
