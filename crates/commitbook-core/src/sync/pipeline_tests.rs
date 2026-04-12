use super::*;
use crate::domain::sync_plan::{DocumentState, PlannedDocumentSync, SyncMode, SyncPlan};
use crate::transport::mock::MockTransport;

fn setup_repo() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path().to_path_buf();
    let cb_dir = repo_root.join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("base")).unwrap();
    (tmp, repo_root, cb_dir)
}

#[tokio::test]
async fn test_execute_noop_returns_zeros() {
    let (_tmp, repo_root, cb_dir) = setup_repo();
    let plan = SyncPlan {
        mode: SyncMode::Noop,
        documents: Vec::new(),
    };
    let transport = MockTransport::new();

    let result =
        execute_sync(&plan, &cb_dir, &repo_root, "main", &transport)
            .await
            .unwrap();

    assert_eq!(result.pulled, 0);
    assert_eq!(result.pushed, 0);
    assert_eq!(result.conflicts, 0);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_execute_pull_writes_files() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

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
        }],
    };

    let transport = MockTransport::new()
        .with_file("notes.md", "# Hello from remote");

    let result =
        execute_sync(&plan, &cb_dir, &repo_root, "main", &transport)
            .await
            .unwrap();

    assert_eq!(result.pulled, 1);

    // File written to working tree.
    let content = std::fs::read_to_string(repo_root.join("notes.md")).unwrap();
    assert_eq!(content, "# Hello from remote");

    // Base version updated.
    let base = crate::state::base::read(&cb_dir, "notes.md").unwrap();
    assert_eq!(base.as_deref(), Some("# Hello from remote"));
}

#[tokio::test]
async fn test_execute_push_reads_working_tree() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // Write a file to the working tree.
    std::fs::write(repo_root.join("local.md"), "# Local content").unwrap();

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
        }],
    };

    let transport = MockTransport::new();

    let result =
        execute_sync(&plan, &cb_dir, &repo_root, "main", &transport)
            .await
            .unwrap();

    assert_eq!(result.pushed, 1);

    // Verify the transport received the file.
    let written = transport.written_files();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].path, "local.md");
    assert_eq!(written[0].content, "# Local content");
}

#[tokio::test]
async fn test_execute_pull_merge_detects_conflict() {
    let (_tmp, repo_root, cb_dir) = setup_repo();

    // Base version.
    crate::state::base::write(&cb_dir, "shared.md", "# Title\n\n## Section\n\nOriginal content\n").unwrap();

    // Local version (modified).
    std::fs::write(
        repo_root.join("shared.md"),
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
        }],
    };

    // Remote version (also modified differently).
    let transport = MockTransport::new()
        .with_file("shared.md", "# Title\n\n## Section\n\nRemote edit\n");

    let result =
        execute_sync(&plan, &cb_dir, &repo_root, "main", &transport)
            .await
            .unwrap();

    // Should detect the conflict in the section.
    assert_eq!(result.pulled, 1);
    assert!(result.conflicts > 0);
}
