use super::*;
use crate::domain::sync_plan::SyncMode;
use crate::git::test_support::setup_repo_with_base;
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

#[tokio::test]
async fn test_plan_noop_no_changes() {
    // Working tree matches the committed base; transport reports same head.
    let fx = setup_repo_with_base(&[("notes.md", "# Hello")]);

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: Some("2026-01-01T00:00:00Z".to_string()),
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_file("notes.md", "# Hello")
        .with_head(&fx.base_sha);

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::Noop);
    assert!(plan.documents.is_empty());
    assert_eq!(plan.base_revision, Some(fx.base_sha));
}

#[tokio::test]
async fn test_plan_pull_only() {
    // Local file matches base (not dirty); remote has moved.
    let fx = setup_repo_with_base(&[("notes.md", "# Hello")]);

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("notes.md", "# Updated remotely");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PullOnly);
    assert!(!plan.documents.is_empty());
    assert!(plan.documents[0].requires_download);
    assert!(!plan.documents[0].requires_upload);
}

#[tokio::test]
async fn test_plan_push_only() {
    // Working tree differs from committed base (dirty); remote unchanged.
    let fx = setup_repo_with_base(&[("notes.md", "# Original")]);
    std::fs::write(fx.repo_root.join("notes.md"), "# Modified locally").unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head(&fx.base_sha)
        .with_file("notes.md", "# Original");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PushOnly);
    let doc = &plan.documents[0];
    assert!(doc.requires_upload);
    assert!(!doc.requires_download);
}

#[tokio::test]
async fn test_plan_pull_then_push() {
    // Both sides changed since base.
    let fx = setup_repo_with_base(&[("notes.md", "# Original")]);
    std::fs::write(fx.repo_root.join("notes.md"), "# Modified locally").unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("notes.md", "# Modified remotely");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PullThenPush);
    let doc = plan.documents.iter().find(|d| d.path == "notes.md").unwrap();
    assert!(doc.requires_merge);
    assert!(doc.requires_download);
    assert!(doc.requires_upload);
}

#[tokio::test]
async fn test_plan_new_remote_file() {
    // No local files committed or in working tree. Remote has a new one.
    let fx = setup_repo_with_base(&[]);

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("new-file.md", "# Brand new");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PullOnly);
    let doc = &plan.documents[0];
    assert_eq!(doc.path, "new-file.md");
    assert_eq!(doc.local_state, DocumentState::Unknown);
    assert_eq!(doc.remote_state, DocumentState::Added);
    assert!(doc.requires_download);
}

#[tokio::test]
async fn test_plan_deleted_on_remote() {
    // File was in base (committed locally) but not on remote anymore.
    // Also remove it from the working tree so it's not "tracked" locally.
    let fx = setup_repo_with_base(&[("deleted.md", "# Was here")]);
    std::fs::remove_file(fx.repo_root.join("deleted.md")).unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    // Remote has no files.
    let transport = MockTransport::new().with_head("new_head");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    let doc = plan
        .documents
        .iter()
        .find(|d| d.path == "deleted.md")
        .unwrap();
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
