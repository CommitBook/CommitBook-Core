use super::*;
use crate::git::test_support::{clone_second_workdir, commit_and_push_from, setup_repo_with_bare_remote};
use std::fs;

fn transport_from_fixture(
    fx: &crate::git::test_support::RepoFixture,
) -> GitRemoteTransport {
    GitRemoteTransport::new(
        fx.repo_dir.path().to_path_buf(),
        "origin".to_string(),
        fx.branch.clone(),
    )
}

#[tokio::test]
async fn test_validate_fetches() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);
    transport.validate().await.unwrap();
}

#[tokio::test]
async fn test_list_repos_single() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);
    let repos = transport.list_repos().await.unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].provider, "git");
}

#[tokio::test]
async fn test_get_head_returns_remote_sha() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);

    let expected = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
    let got = transport.get_head("ignored").await.unwrap();
    assert_eq!(got, expected);
}

#[tokio::test]
async fn test_get_head_follows_remote_not_local_head() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);

    // A second workdir pushes a new commit. Our HEAD stays where it is,
    // but get_head should see the remote's new commit.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");

    let local_head = fx.repo.rev_parse("HEAD").unwrap();
    let got = transport.get_head("ignored").await.unwrap();
    assert_ne!(got, local_head);
}

#[tokio::test]
async fn test_list_files_reads_from_ref_not_working_tree() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);

    // Create an uncommitted markdown file in the working tree.
    fs::write(fx.repo_dir.path().join("uncommitted.md"), "# nope\n").unwrap();

    let files = transport.list_files("ignored").await.unwrap();
    assert_eq!(files, vec!["init.md".to_string()]);
}

#[tokio::test]
async fn test_list_files_filters_to_markdown() {
    let fx = setup_repo_with_bare_remote();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "notes.md", "# notes\n");
    commit_and_push_from(other.path(), &fx.branch, "image.png", "fake");
    commit_and_push_from(other.path(), &fx.branch, "script.sh", "#!/bin/bash\n");

    let transport = transport_from_fixture(&fx);
    let files = transport.list_files("ignored").await.unwrap();
    assert!(files.contains(&"init.md".to_string()));
    assert!(files.contains(&"notes.md".to_string()));
    assert!(!files.iter().any(|f| f.ends_with(".png")));
    assert!(!files.iter().any(|f| f.ends_with(".sh")));
}

#[tokio::test]
async fn test_read_file_returns_committed_content() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);

    // Dirty the working tree.
    fs::write(fx.repo_dir.path().join("init.md"), "UNCOMMITTED\n").unwrap();

    let doc = transport.read_file("ignored", "init.md").await.unwrap();
    assert_eq!(doc.content, "# init\n");
    assert_eq!(doc.path, "init.md");
    assert!(!doc.revision.is_empty());
}

#[tokio::test]
async fn test_write_files_creates_commit_and_pushes() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);

    let head_before = fx.repo.rev_parse("HEAD").unwrap();

    let results = transport
        .write_files(
            "ignored",
            vec![WriteFileInput {
                path: "added.md".to_string(),
                content: "# added\n".to_string(),
                message: "Add added.md".to_string(),
                base_revision: None,
            }],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);

    let head_after = fx.repo.rev_parse("HEAD").unwrap();
    assert_ne!(head_before, head_after);

    // Remote advanced to the same SHA.
    let remote_after = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!(head_after, remote_after);
}

#[tokio::test]
async fn test_write_files_unchanged_skips_commit_and_push() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);

    let head_before = fx.repo.rev_parse("HEAD").unwrap();

    let results = transport
        .write_files(
            "ignored",
            vec![WriteFileInput {
                path: "init.md".to_string(),
                content: "# init\n".to_string(),
                message: "No-op".to_string(),
                base_revision: None,
            }],
        )
        .await
        .unwrap();

    assert!(results.is_empty());
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
}

#[tokio::test]
async fn test_write_files_recovers_from_non_ff_push() {
    let fx = setup_repo_with_bare_remote();
    let transport = transport_from_fixture(&fx);

    // Another workdir pushes a commit on a different file between our last
    // sync and this write — simulates "remote moved while we were editing".
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote_only.md", "# remote\n");

    // Our write. Internally: local commit → push rejected → fetch → rebase → push.
    let results = transport
        .write_files(
            "ignored",
            vec![WriteFileInput {
                path: "ours.md".to_string(),
                content: "# ours\n".to_string(),
                message: "Add ours".to_string(),
                base_revision: None,
            }],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);

    // After recovery, both commits are reachable on our HEAD and on origin.
    let head_files = fx.repo.ls_tree_files("HEAD").unwrap();
    assert!(head_files.contains(&"init.md".to_string()));
    assert!(head_files.contains(&"remote_only.md".to_string()));
    assert!(head_files.contains(&"ours.md".to_string()));

    let head_sha = fx.repo.rev_parse("HEAD").unwrap();
    let remote_sha = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!(head_sha, remote_sha);
}
