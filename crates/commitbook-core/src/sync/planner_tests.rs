use super::*;
use crate::domain::sync_plan::SyncMode;
use crate::transport::mock::MockTransport;

// --- Helper function tests ---

#[test]
fn test_collect_markdown_files_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    collect_markdown_files(tmp.path(), tmp.path(), &mut files).unwrap();
    assert!(files.is_empty());
}

#[test]
fn test_collect_markdown_files_finds_md() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.md"), "# Notes").unwrap();
    std::fs::write(tmp.path().join("todo.md"), "# Todo").unwrap();

    let mut files = Vec::new();
    collect_markdown_files(tmp.path(), tmp.path(), &mut files).unwrap();
    files.sort();
    assert_eq!(files, vec!["notes.md", "todo.md"]);
}

#[test]
fn test_collect_markdown_files_skips_hidden() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("visible.md"), "visible").unwrap();
    let hidden = tmp.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("secret.md"), "secret").unwrap();

    let mut files = Vec::new();
    collect_markdown_files(tmp.path(), tmp.path(), &mut files).unwrap();
    assert_eq!(files, vec!["visible.md"]);
}

#[test]
fn test_collect_markdown_files_skips_non_markdown() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("code.rs"), "fn main() {}").unwrap();
    std::fs::write(tmp.path().join("data.json"), "{}").unwrap();
    std::fs::write(tmp.path().join("notes.md"), "# Notes").unwrap();

    let mut files = Vec::new();
    collect_markdown_files(tmp.path(), tmp.path(), &mut files).unwrap();
    assert_eq!(files, vec!["notes.md"]);
}

#[test]
fn test_collect_markdown_files_finds_markdown_extension() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("doc.markdown"), "# Doc").unwrap();

    let mut files = Vec::new();
    collect_markdown_files(tmp.path(), tmp.path(), &mut files).unwrap();
    assert_eq!(files, vec!["doc.markdown"]);
}

#[test]
fn test_collect_markdown_files_nested_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("nested.md"), "# Nested").unwrap();

    let mut files = Vec::new();
    collect_markdown_files(tmp.path(), tmp.path(), &mut files).unwrap();
    assert_eq!(files, vec!["subdir/nested.md"]);
}

#[test]
fn test_list_tracked_files_default_patterns() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.md"), "a").unwrap();
    std::fs::write(tmp.path().join("b.rs"), "b").unwrap();

    let mut files = list_tracked_files(tmp.path(), &[]).unwrap();
    files.sort();
    assert_eq!(files, vec!["a.md"]);
}

// --- create_sync_plan tests ---

fn setup_repo() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path().to_path_buf();
    let cb_dir = repo_root.join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local").join("base")).unwrap();
    (tmp, repo_root, cb_dir.clone())
}

#[tokio::test]
async fn test_plan_noop_no_changes() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // Write a file + matching base version.
    std::fs::write(repo_root.join("notes.md"), "# Hello").unwrap();
    crate::state::base::write(&cb_dir, "notes.md", "# Hello").unwrap();

    // Set checkpoint to same head.
    let state = crate::state::sync_state::SyncState {
        remote_head: Some("mock_head_sha_000".to_string()),
        last_sync_at: Some("2026-01-01T00:00:00Z".to_string()),
    };
    state.save(&cb_dir).unwrap();

    let transport = MockTransport::new().with_file("notes.md", "# Hello");
    let plan = create_sync_plan(&cb_dir, &repo_root, "main", &transport, &[]).await.unwrap();

    assert_eq!(plan.mode, SyncMode::Noop);
    assert!(plan.documents.is_empty());
}

#[tokio::test]
async fn test_plan_pull_only() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // Local file matches base (not dirty).
    std::fs::write(repo_root.join("notes.md"), "# Hello").unwrap();
    crate::state::base::write(&cb_dir, "notes.md", "# Hello").unwrap();

    // Checkpoint head differs from transport head → remote changed.
    let state = crate::state::sync_state::SyncState {
        remote_head: Some("old_head".to_string()),
        last_sync_at: None,
    };
    state.save(&cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("notes.md", "# Updated remotely");

    let plan = create_sync_plan(&cb_dir, &repo_root, "main", &transport, &[]).await.unwrap();

    assert_eq!(plan.mode, SyncMode::PullOnly);
    assert!(!plan.documents.is_empty());
    assert!(plan.documents[0].requires_download);
    assert!(!plan.documents[0].requires_upload);
}

#[tokio::test]
async fn test_plan_push_only() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // Local file differs from base (dirty).
    std::fs::write(repo_root.join("notes.md"), "# Modified locally").unwrap();
    crate::state::base::write(&cb_dir, "notes.md", "# Original").unwrap();

    // Checkpoint matches transport head → remote unchanged.
    let state = crate::state::sync_state::SyncState {
        remote_head: Some("same_head".to_string()),
        last_sync_at: None,
    };
    state.save(&cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("same_head")
        .with_file("notes.md", "# Original");

    let plan = create_sync_plan(&cb_dir, &repo_root, "main", &transport, &[]).await.unwrap();

    assert_eq!(plan.mode, SyncMode::PushOnly);
    let doc = &plan.documents[0];
    assert!(doc.requires_upload);
    assert!(!doc.requires_download);
}

#[tokio::test]
async fn test_plan_pull_then_push() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // Dirty local file.
    std::fs::write(repo_root.join("notes.md"), "# Modified locally").unwrap();
    crate::state::base::write(&cb_dir, "notes.md", "# Original").unwrap();

    // Remote also changed.
    let state = crate::state::sync_state::SyncState {
        remote_head: Some("old_head".to_string()),
        last_sync_at: None,
    };
    state.save(&cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("notes.md", "# Modified remotely");

    let plan = create_sync_plan(&cb_dir, &repo_root, "main", &transport, &[]).await.unwrap();

    assert_eq!(plan.mode, SyncMode::PullThenPush);
    let doc = plan.documents.iter().find(|d| d.path == "notes.md").unwrap();
    assert!(doc.requires_merge);
    assert!(doc.requires_download);
    assert!(doc.requires_upload);
}

#[tokio::test]
async fn test_plan_new_remote_file() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // No local files, no base. Remote has a new file.
    let state = crate::state::sync_state::SyncState {
        remote_head: Some("old_head".to_string()),
        last_sync_at: None,
    };
    state.save(&cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("new-file.md", "# Brand new");

    let plan = create_sync_plan(&cb_dir, &repo_root, "main", &transport, &[]).await.unwrap();

    assert_eq!(plan.mode, SyncMode::PullOnly);
    let doc = &plan.documents[0];
    assert_eq!(doc.path, "new-file.md");
    assert_eq!(doc.local_state, DocumentState::Unknown);
    assert_eq!(doc.remote_state, DocumentState::Added);
    assert!(doc.requires_download);
}

#[tokio::test]
async fn test_plan_deleted_on_remote() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // File exists in base but not on remote.
    crate::state::base::write(&cb_dir, "deleted.md", "# Was here").unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some("old_head".to_string()),
        last_sync_at: None,
    };
    state.save(&cb_dir).unwrap();

    // Remote has no files.
    let transport = MockTransport::new().with_head("new_head");

    let plan = create_sync_plan(&cb_dir, &repo_root, "main", &transport, &[]).await.unwrap();

    let doc = plan.documents.iter().find(|d| d.path == "deleted.md").unwrap();
    assert!(doc.requires_delete);
}

#[test]
fn test_list_tracked_files_invalid_glob_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    // "[invalid" is an unclosed bracket — glob::glob returns PatternError for it.
    let result = list_tracked_files(tmp.path(), &["[invalid".to_string()]);
    assert!(result.is_err(), "expected Err for invalid glob pattern, got Ok");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("Invalid glob pattern"), "unexpected error message: {msg}");
}
