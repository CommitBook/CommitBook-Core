use super::*;
use crate::domain::sync_plan::SyncMode;
use crate::git::test_support::setup_repo_with_base;
use crate::transport::mock::MockTransport;

#[tokio::test]
async fn test_plan_noop_no_changes() {
    // Working tree matches the committed base; transport reports same head.
    let fx = setup_repo_with_base(&[("notes.md", "# Hello\n")]);

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: Some("2026-01-01T00:00:00Z".to_string()),
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_file("notes.md", "# Hello\n")
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
    let fx = setup_repo_with_base(&[("notes.md", "# Hello\n")]);

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("notes.md", "# Updated remotely\n");

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
    let fx = setup_repo_with_base(&[("notes.md", "# Original\n")]);
    std::fs::write(fx.repo_root.join("notes.md"), "# Modified locally\n").unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head(&fx.base_sha)
        .with_file("notes.md", "# Original\n");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PushOnly);
    let doc = plan
        .documents
        .iter()
        .find(|d| d.path == "notes.md")
        .unwrap();
    assert!(doc.requires_upload);
    assert!(!doc.requires_download);
}

#[tokio::test]
async fn test_plan_pull_then_push() {
    // Both sides changed since base.
    let fx = setup_repo_with_base(&[("notes.md", "# Original\n")]);
    std::fs::write(fx.repo_root.join("notes.md"), "# Modified locally\n").unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("notes.md", "# Modified remotely\n");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PullThenPush);
    let doc = plan
        .documents
        .iter()
        .find(|d| d.path == "notes.md")
        .unwrap();
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
        .with_file("new-file.md", "# Brand new\n");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PullOnly);
    let doc = plan
        .documents
        .iter()
        .find(|d| d.path == "new-file.md")
        .unwrap();
    assert_eq!(doc.local_state, DocumentState::Unknown);
    assert_eq!(doc.remote_state, DocumentState::Added);
    assert!(doc.requires_download);
}

#[tokio::test]
async fn test_plan_deleted_on_remote() {
    // File exists locally (both in base and working tree) but remote dropped it.
    // Planner should emit a requires_delete entry so the pull phase removes it
    // from the working tree.
    let fx = setup_repo_with_base(&[("doomed.md", "# Was here\n")]);

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    // Remote has no markdown files.
    let transport = MockTransport::new().with_head("new_head");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    let doc = plan
        .documents
        .iter()
        .find(|d| d.path == "doomed.md")
        .unwrap();
    assert!(doc.requires_delete);
    assert!(!doc.requires_remote_delete);
}

#[tokio::test]
async fn test_plan_local_delete_pushes_delete() {
    // File was in the base commit and is still on remote. User deleted it
    // locally. Planner must emit requires_remote_delete so the pipeline pushes
    // the deletion.
    let fx = setup_repo_with_base(&[("stale.md", "# stale\n")]);
    std::fs::remove_file(fx.repo_root.join("stale.md")).unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    // Remote still has the file at its own head (which differs from ours).
    let transport = MockTransport::new()
        .with_head("new_head")
        .with_file("stale.md", "# stale\n");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PullThenPush);
    let doc = plan
        .documents
        .iter()
        .find(|d| d.path == "stale.md")
        .unwrap();
    assert!(doc.requires_remote_delete);
    assert!(!doc.requires_delete);
    assert!(!doc.requires_upload);
    assert_eq!(doc.local_state, DocumentState::Deleted);
}

#[tokio::test]
async fn test_plan_local_delete_pushes_delete_when_remote_unchanged() {
    // Same scenario but remote hasn't moved — we still need to push the delete.
    let fx = setup_repo_with_base(&[("stale.md", "# stale\n")]);
    std::fs::remove_file(fx.repo_root.join("stale.md")).unwrap();

    let state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: None,
    };
    state.save(&fx.cb_dir).unwrap();

    // Mock transport at same SHA as checkpoint — remote unchanged.
    let transport = MockTransport::new()
        .with_head(&fx.base_sha)
        .with_file("stale.md", "# stale\n");

    let plan = create_sync_plan(&fx.cb_dir, &fx.repo_root, "main", &transport, &[])
        .await
        .unwrap();

    assert_eq!(plan.mode, SyncMode::PushOnly);
    let doc = plan
        .documents
        .iter()
        .find(|d| d.path == "stale.md")
        .unwrap();
    assert!(doc.requires_remote_delete);
}
