use super::*;
use crate::git::test_support::setup_repo_with_bare_remote;

#[test]
fn test_read_returns_committed_content_at_sha() {
    let fx = setup_repo_with_bare_remote();
    let sha = fx.repo.rev_parse("HEAD").unwrap();
    let content = read(&fx.repo, &Some(sha), "init.md").unwrap();
    assert_eq!(content, "# init\n");
}

#[test]
fn test_read_returns_none_when_path_missing_at_sha() {
    let fx = setup_repo_with_bare_remote();
    let sha = fx.repo.rev_parse("HEAD").unwrap();
    assert!(read(&fx.repo, &Some(sha), "missing.md").is_none());
}

#[test]
fn test_read_returns_none_when_base_sha_is_none() {
    let fx = setup_repo_with_bare_remote();
    assert!(read(&fx.repo, &None, "init.md").is_none());
}

#[test]
fn test_read_returns_none_for_invalid_sha() {
    let fx = setup_repo_with_bare_remote();
    let bogus = Some("0000000000000000000000000000000000000000".to_string());
    assert!(read(&fx.repo, &bogus, "init.md").is_none());
}
