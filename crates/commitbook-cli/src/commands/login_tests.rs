use super::*;

#[test]
fn test_detect_provider_no_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(detect_provider(tmp.path()), "generic");
}

#[test]
fn test_detect_provider_no_remote() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "generic");
}

#[test]
fn test_detect_provider_github() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["remote", "add", "origin", "https://github.com/user/repo.git"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "github");
}

#[test]
fn test_detect_provider_gitlab() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["remote", "add", "origin", "https://gitlab.com/user/repo.git"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "gitlab");
}

#[test]
fn test_detect_provider_codeberg() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["remote", "add", "origin", "https://codeberg.org/user/repo.git"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "codeberg");
}
