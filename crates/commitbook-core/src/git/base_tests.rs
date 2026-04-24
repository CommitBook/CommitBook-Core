use super::*;
use crate::git::test_support::{commit_and_push_from, setup_repo_with_bare_remote};

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

#[test]
fn test_list_returns_markdown_paths_at_sha() {
    let fx = setup_repo_with_bare_remote();

    // Push additional files on a second workdir so the base SHA has multiple paths.
    let other = crate::git::test_support::clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "notes.md", "# notes\n");
    commit_and_push_from(other.path(), &fx.branch, "docs/guide.md", "# guide\n");
    fx.repo.fetch("origin", &fx.branch).unwrap();

    let sha = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
    let mut files = list(&fx.repo, &Some(sha)).unwrap();
    files.sort();
    assert_eq!(
        files,
        vec![
            "docs/guide.md".to_string(),
            "init.md".to_string(),
            "notes.md".to_string(),
        ]
    );
}

#[test]
fn test_list_filters_non_markdown_and_hidden() {
    let fx = setup_repo_with_bare_remote();
    let other = crate::git::test_support::clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "notes.md", "# notes\n");
    commit_and_push_from(other.path(), &fx.branch, "image.png", "fake");
    commit_and_push_from(other.path(), &fx.branch, ".hidden/secret.md", "shhh");
    fx.repo.fetch("origin", &fx.branch).unwrap();

    let sha = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
    let files = list(&fx.repo, &Some(sha)).unwrap();
    assert!(files.contains(&"init.md".to_string()));
    assert!(files.contains(&"notes.md".to_string()));
    assert!(!files.iter().any(|f| f.ends_with(".png")));
    assert!(!files.iter().any(|f| f.contains(".hidden")));
}

#[test]
fn test_list_returns_empty_when_base_sha_is_none() {
    let fx = setup_repo_with_bare_remote();
    let files = list(&fx.repo, &None).unwrap();
    assert!(files.is_empty());
}
