use super::*;
use crate::domain::sync_plan::{DocumentState, PlannedDocumentSync, SyncMode, SyncPlan};
use crate::git::test_support::setup_repo_with_base;
use crate::logger::FileLogger;
use crate::transport::mock::MockTransport;

fn test_logger(repo_root: &std::path::Path) -> FileLogger {
    FileLogger::new(repo_root, 30).unwrap()
}

#[tokio::test]
async fn test_execute_noop_returns_zeros() {
    let fx = setup_repo_with_base(&[]);
    let plan = SyncPlan {
        mode: SyncMode::Noop,
        documents: Vec::new(),
        base_revision: None,
    };
    let transport = MockTransport::new();

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.pulled, 0);
    assert_eq!(result.pushed(), 0);
    assert_eq!(result.conflicts, 0);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_execute_pull_writes_files() {
    // Brand-new repo on remote side — no existing base.
    let fx = setup_repo_with_base(&[]);

    let plan = SyncPlan {
        mode: SyncMode::PullOnly,
        documents: vec![PlannedDocumentSync {
            path: "notes.md".to_string(),
            local_state: DocumentState::Unknown,
            remote_state: DocumentState::Added,
            requires_merge: false,
            requires_conflict: false,
            requires_upload: false,
            requires_download: true,
            requires_delete: false,
            requires_remote_delete: false,
        }],
        base_revision: None,
    };

    let transport = MockTransport::new().with_file("notes.md", "# Hello from remote");

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.pulled, 1);

    // File written to working tree.
    let content = std::fs::read_to_string(fx.repo_root.join("notes.md")).unwrap();
    assert_eq!(content, "# Hello from remote");

    // Checkpoint advanced to the remote head (the "new base" for the next sync).
    let state = crate::state::sync_state::SyncState::load(&fx.cb_dir).unwrap();
    assert_eq!(state.remote_head.as_deref(), Some("mock_head_sha_000"));
}

#[tokio::test]
async fn test_execute_push_reads_working_tree() {
    let fx = setup_repo_with_base(&[]);

    // Write a file to the working tree that isn't in the base commit.
    std::fs::write(fx.repo_root.join("local.md"), "# Local content").unwrap();

    let plan = SyncPlan {
        mode: SyncMode::PushOnly,
        documents: vec![PlannedDocumentSync {
            path: "local.md".to_string(),
            local_state: DocumentState::Modified,
            remote_state: DocumentState::Unknown,
            requires_merge: false,
            requires_conflict: false,
            requires_upload: true,
            requires_download: false,
            requires_delete: false,
            requires_remote_delete: false,
        }],
        base_revision: Some(fx.base_sha.clone()),
    };

    let transport = MockTransport::new();

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.pushed(), 1);

    // Verify the transport received the file.
    let written = transport.written_files();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].path, "local.md");
    assert_eq!(written[0].content, "# Local content");
}

#[tokio::test]
async fn test_execute_pull_merge_detects_conflict() {
    // Base is committed to git. Local has edited the working tree.
    let fx = setup_repo_with_base(&[(
        "shared.md",
        "# Title\n\n## Section\n\nOriginal content\n",
    )]);
    std::fs::write(
        fx.repo_root.join("shared.md"),
        "# Title\n\n## Section\n\nLocal edit\n",
    )
    .unwrap();

    let plan = SyncPlan {
        mode: SyncMode::PullThenPush,
        documents: vec![PlannedDocumentSync {
            path: "shared.md".to_string(),
            local_state: DocumentState::Modified,
            remote_state: DocumentState::Modified,
            requires_merge: true,
            requires_conflict: false,
            requires_upload: true,
            requires_download: true,
            requires_delete: false,
            requires_remote_delete: false,
        }],
        base_revision: Some(fx.base_sha.clone()),
    };

    // Remote edit conflicts with local on the same section.
    let transport = MockTransport::new()
        .with_file("shared.md", "# Title\n\n## Section\n\nRemote edit\n");

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.pulled, 1);
    assert!(result.conflicts > 0);
}

#[tokio::test]
async fn test_pull_then_push_failure_does_not_advance_checkpoint() {
    // Base committed; local edit on top; remote has divergent content; push fails.
    let fx = setup_repo_with_base(&[("shared.md", "# Original\n")]);
    std::fs::write(fx.repo_root.join("shared.md"), "# Local edit\n").unwrap();

    let initial_state = crate::state::sync_state::SyncState {
        remote_head: Some(fx.base_sha.clone()),
        last_sync_at: Some("2024-01-01T00:00:00Z".to_string()),
    };
    initial_state.save(&fx.cb_dir).unwrap();

    let plan = SyncPlan {
        mode: SyncMode::PullThenPush,
        documents: vec![PlannedDocumentSync {
            path: "shared.md".to_string(),
            local_state: DocumentState::Modified,
            remote_state: DocumentState::Modified,
            requires_merge: true,
            requires_conflict: false,
            requires_upload: true,
            requires_download: true,
            requires_delete: false,
            requires_remote_delete: false,
        }],
        base_revision: Some(fx.base_sha.clone()),
    };

    let transport = MockTransport::new()
        .with_file("shared.md", "# Remote edit\n")
        .with_head("new_sha")
        .with_fail_writes();

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    // Push failed.
    assert!(!result.errors.is_empty());
    assert_eq!(result.pushed(), 0);

    // Checkpoint must still point at the pre-sync SHA — we never advanced
    // past a push that didn't land.
    let state = crate::state::sync_state::SyncState::load(&fx.cb_dir).unwrap();
    assert_eq!(state.remote_head, Some(fx.base_sha));
}

#[tokio::test]
async fn test_push_only_failure_does_not_advance_checkpoint() {
    let fx = setup_repo_with_base(&[]);

    let initial_state = crate::state::sync_state::SyncState {
        remote_head: Some("old_sha".to_string()),
        last_sync_at: Some("2024-01-01T00:00:00Z".to_string()),
    };
    initial_state.save(&fx.cb_dir).unwrap();

    std::fs::write(fx.repo_root.join("file.md"), "# Content").unwrap();

    let plan = SyncPlan {
        mode: SyncMode::PushOnly,
        documents: vec![PlannedDocumentSync {
            path: "file.md".to_string(),
            local_state: DocumentState::Modified,
            remote_state: DocumentState::Unknown,
            requires_merge: false,
            requires_conflict: false,
            requires_upload: true,
            requires_download: false,
            requires_delete: false,
            requires_remote_delete: false,
        }],
        base_revision: Some("old_sha".to_string()),
    };

    let transport = MockTransport::new()
        .with_head("new_sha")
        .with_fail_writes();

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert!(!result.errors.is_empty());

    let state = crate::state::sync_state::SyncState::load(&fx.cb_dir).unwrap();
    assert_eq!(state.remote_head, Some("old_sha".to_string()));
}

#[tokio::test]
async fn test_pull_then_push_success_advances_checkpoint() {
    // Base committed; local edit; remote diverged — full merge + push.
    let fx = setup_repo_with_base(&[("doc.md", "# Base\n")]);
    std::fs::write(fx.repo_root.join("doc.md"), "# Local\n").unwrap();

    let plan = SyncPlan {
        mode: SyncMode::PullThenPush,
        documents: vec![PlannedDocumentSync {
            path: "doc.md".to_string(),
            local_state: DocumentState::Modified,
            remote_state: DocumentState::Modified,
            requires_merge: true,
            requires_conflict: false,
            requires_upload: true,
            requires_download: true,
            requires_delete: false,
            requires_remote_delete: false,
        }],
        base_revision: Some(fx.base_sha.clone()),
    };

    let transport = MockTransport::new()
        .with_file("doc.md", "# Remote\n")
        .with_head("new_head_sha");

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert!(result.errors.is_empty());
    assert!(result.pushed() > 0);

    // Checkpoint advanced to the new head; that SHA is the new base for the
    // next sync cycle.
    let state = crate::state::sync_state::SyncState::load(&fx.cb_dir).unwrap();
    assert_eq!(state.remote_head.as_deref(), Some("new_head_sha"));
}

#[tokio::test]
async fn test_push_phase_deletes_on_remote() {
    let fx = setup_repo_with_base(&[("stale.md", "# stale\n")]);

    let plan = SyncPlan {
        mode: SyncMode::PushOnly,
        documents: vec![PlannedDocumentSync {
            path: "stale.md".to_string(),
            local_state: DocumentState::Deleted,
            remote_state: DocumentState::Modified,
            requires_merge: false,
            requires_conflict: false,
            requires_upload: false,
            requires_download: false,
            requires_delete: false,
            requires_remote_delete: true,
        }],
        base_revision: Some(fx.base_sha.clone()),
    };

    let transport = MockTransport::new();

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        Some("Delete stale notes".to_string()),
    )
    .await
    .unwrap();

    assert!(result.errors.is_empty());
    assert_eq!(result.pushed(), 1);

    let deletes = transport.deleted_files();
    assert_eq!(deletes.len(), 1);
    assert_eq!(deletes[0].0, "stale.md");
    assert_eq!(deletes[0].1, "Delete stale notes");
}

#[tokio::test]
async fn test_pull_only_advances_checkpoint() {
    let fx = setup_repo_with_base(&[]);

    let plan = SyncPlan {
        mode: SyncMode::PullOnly,
        documents: vec![PlannedDocumentSync {
            path: "remote.md".to_string(),
            local_state: DocumentState::Unknown,
            remote_state: DocumentState::Added,
            requires_merge: false,
            requires_conflict: false,
            requires_upload: false,
            requires_download: true,
            requires_delete: false,
            requires_remote_delete: false,
        }],
        base_revision: None,
    };

    let transport = MockTransport::new()
        .with_file("remote.md", "# From remote\n")
        .with_head("post_pull_sha");

    let logger = test_logger(&fx.repo_root);
    let result = execute_sync(
        &plan,
        &fx.cb_dir,
        &fx.repo_root,
        "main",
        &transport,
        &logger,
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.pulled, 1);
    assert!(result.errors.is_empty());

    // Checkpoint should point at the remote head post-pull.
    let state = crate::state::sync_state::SyncState::load(&fx.cb_dir).unwrap();
    assert_eq!(state.remote_head.as_deref(), Some("post_pull_sha"));
}
