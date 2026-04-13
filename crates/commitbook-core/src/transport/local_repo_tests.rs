use super::*;
use git2::{Repository, Signature};
use std::fs;
use tempfile::tempdir;

/// Get the default branch name after initial commit (may be "main" or "master").
fn default_branch(repo: &Repository) -> String {
    repo.head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "main".to_string())
}

fn setup_test_repo() -> (tempfile::TempDir, LocalRepoTransport) {
    let tmp = tempdir().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    // Configure user for commits.
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test User").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();

    // Create initial commit with a markdown file.
    fs::write(tmp.path().join("notes.md"), "# Notes\n\nHello world.\n").unwrap();
    fs::write(tmp.path().join("todo.md"), "# Todo\n\n- Buy milk\n").unwrap();
    fs::create_dir_all(tmp.path().join("docs")).unwrap();
    fs::write(
        tmp.path().join("docs/guide.md"),
        "# Guide\n\nWelcome.\n",
    )
    .unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(Path::new("notes.md")).unwrap();
    index.add_path(Path::new("todo.md")).unwrap();
    index.add_path(Path::new("docs/guide.md")).unwrap();
    index.write().unwrap();

    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = Signature::now("Test User", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();

    let transport = LocalRepoTransport::new(tmp.path().to_path_buf(), default_branch(&repo));
    (tmp, transport)
}

#[tokio::test]
async fn test_validate() {
    let (_tmp, transport) = setup_test_repo();
    transport.validate().await.unwrap();
}

#[tokio::test]
async fn test_validate_bad_branch() {
    let (_tmp, mut transport) = setup_test_repo();
    transport.branch = "nonexistent".to_string();
    assert!(transport.validate().await.is_err());
}

#[tokio::test]
async fn test_list_repos() {
    let (_tmp, transport) = setup_test_repo();
    let repos = transport.list_repos().await.unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].provider, "local");
}

#[tokio::test]
async fn test_list_files() {
    let (_tmp, transport) = setup_test_repo();
    let files = transport.list_files("main").await.unwrap();
    assert_eq!(files.len(), 3);
    assert!(files.contains(&"notes.md".to_string()));
    assert!(files.contains(&"todo.md".to_string()));
    assert!(files.contains(&"docs/guide.md".to_string()));
}

#[tokio::test]
async fn test_list_files_ignores_hidden() {
    let (tmp, transport) = setup_test_repo();
    // Create a hidden directory with markdown files — should be ignored.
    fs::create_dir_all(tmp.path().join(".hidden")).unwrap();
    fs::write(tmp.path().join(".hidden/secret.md"), "# Secret\n").unwrap();

    let files = transport.list_files("main").await.unwrap();
    assert!(!files.iter().any(|f| f.contains("secret")));
}

#[tokio::test]
async fn test_list_files_ignores_non_markdown() {
    let (tmp, transport) = setup_test_repo();
    fs::write(tmp.path().join("image.png"), b"fake png data").unwrap();
    fs::write(tmp.path().join("script.sh"), "#!/bin/bash\n").unwrap();

    let files = transport.list_files("main").await.unwrap();
    assert!(!files.iter().any(|f| f.contains("png") || f.contains("sh")));
}

#[tokio::test]
async fn test_read_file() {
    let (_tmp, transport) = setup_test_repo();
    let doc = transport.read_file("main", "notes.md").await.unwrap();
    assert_eq!(doc.path, "notes.md");
    assert!(doc.content.contains("# Notes"));
    assert!(doc.content.contains("Hello world."));
    assert!(!doc.revision.is_empty());
}

#[tokio::test]
async fn test_read_file_nested() {
    let (_tmp, transport) = setup_test_repo();
    let doc = transport.read_file("main", "docs/guide.md").await.unwrap();
    assert!(doc.content.contains("# Guide"));
}

#[tokio::test]
async fn test_read_file_not_found() {
    let (_tmp, transport) = setup_test_repo();
    assert!(transport.read_file("main", "nonexistent.md").await.is_err());
}

#[tokio::test]
async fn test_write_files() {
    let (_tmp, transport) = setup_test_repo();

    let results = transport
        .write_files(
            "main",
            vec![WriteFileInput {
                path: "new_file.md".to_string(),
                content: "# New\n\nNew content.\n".to_string(),
                message: "Add new file".to_string(),
                base_revision: None,
            }],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "new_file.md");
    assert!(!results[0].new_revision.is_empty());

    // Verify file was written.
    let doc = transport.read_file("main", "new_file.md").await.unwrap();
    assert!(doc.content.contains("New content."));
}

#[tokio::test]
async fn test_write_files_updates_existing() {
    let (_tmp, transport) = setup_test_repo();

    transport
        .write_files(
            "main",
            vec![WriteFileInput {
                path: "notes.md".to_string(),
                content: "# Notes\n\nUpdated content.\n".to_string(),
                message: "Update notes".to_string(),
                base_revision: None,
            }],
        )
        .await
        .unwrap();

    let doc = transport.read_file("main", "notes.md").await.unwrap();
    assert!(doc.content.contains("Updated content."));
}

#[tokio::test]
async fn test_write_files_multiple() {
    let (_tmp, transport) = setup_test_repo();

    let results = transport
        .write_files(
            "main",
            vec![
                WriteFileInput {
                    path: "a.md".to_string(),
                    content: "# A\n".to_string(),
                    message: "Add a".to_string(),
                    base_revision: None,
                },
                WriteFileInput {
                    path: "b.md".to_string(),
                    content: "# B\n".to_string(),
                    message: "Add b".to_string(),
                    base_revision: None,
                },
            ],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    // Both should share the same commit revision (atomic).
    assert_eq!(results[0].new_revision, results[1].new_revision);
}

#[tokio::test]
async fn test_delete_file() {
    let (tmp, transport) = setup_test_repo();

    transport
        .delete_file("main", "todo.md", "Remove todo")
        .await
        .unwrap();

    assert!(!tmp.path().join("todo.md").exists());
    let files = transport.list_files("main").await.unwrap();
    assert!(!files.contains(&"todo.md".to_string()));
}

#[tokio::test]
async fn test_get_head() {
    let (_tmp, transport) = setup_test_repo();
    let head = transport.get_head("main").await.unwrap();
    assert!(!head.is_empty());
    assert_eq!(head.len(), 40); // SHA-1 hex
}

#[tokio::test]
async fn test_head_changes_after_write() {
    let (_tmp, transport) = setup_test_repo();
    let head_before = transport.get_head("main").await.unwrap();

    transport
        .write_files(
            "main",
            vec![WriteFileInput {
                path: "change.md".to_string(),
                content: "# Change\n".to_string(),
                message: "New commit".to_string(),
                base_revision: None,
            }],
        )
        .await
        .unwrap();

    let head_after = transport.get_head("main").await.unwrap();
    assert_ne!(head_before, head_after);
}

#[tokio::test]
async fn test_write_files_unchanged_skips_commit() {
    let (_tmp, transport) = setup_test_repo();
    let head_before = transport.get_head("main").await.unwrap();

    // Write the exact same content that already exists.
    let results = transport
        .write_files(
            "main",
            vec![WriteFileInput {
                path: "notes.md".to_string(),
                content: "# Notes\n\nHello world.\n".to_string(),
                message: "No-op update".to_string(),
                base_revision: None,
            }],
        )
        .await
        .unwrap();

    assert!(results.is_empty(), "Expected empty results for unchanged tree");

    let head_after = transport.get_head("main").await.unwrap();
    assert_eq!(head_before, head_after, "HEAD should not change for no-op write");
}
