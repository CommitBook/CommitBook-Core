use super::*;

fn create_temp_repo() -> (tempfile::TempDir, git2::Repository) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    (tmp, repo)
}

#[test]
fn test_check_remote_connectivity_no_remote() {
    let (tmp, _repo) = create_temp_repo();
    // No remote configured → should return Ok(false) or Err
    let result = check_remote_connectivity(tmp.path());
    // An error is also acceptable when there is no remote to connect to.
    if let Ok(connected) = result {
        assert!(!connected);
    }
}

#[test]
fn test_get_remote_url_no_remote_errors() {
    let (tmp, _repo) = create_temp_repo();
    let result = get_remote_url(tmp.path(), "origin");
    assert!(result.is_err());
}

#[test]
fn test_get_remote_url_with_remote() {
    let (tmp, repo) = create_temp_repo();
    repo.remote("origin", "https://example.com/repo.git")
        .unwrap();

    let url = get_remote_url(tmp.path(), "origin").unwrap();
    assert_eq!(url, "https://example.com/repo.git");
}

#[test]
fn test_check_remote_connectivity_not_a_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_path = tmp.path().join("nonexistent");
    let result = check_remote_connectivity(&fake_path);
    assert!(result.is_err());
}
