use super::*;
use crate::domain::document::Document;
use crate::domain::sync_plan::{DocumentState, SyncCheckpoint, SyncMode};
use crate::domain::transport::*;
use crate::domain::workspace::*;
use crate::storage::db;
use anyhow::Result;
use async_trait::async_trait;

// --- Mock Transport ---

struct MockTransport {
    head: String,
    files: Vec<String>,
}

#[async_trait]
impl RemoteTransport for MockTransport {
    async fn validate(&self) -> Result<()> {
        Ok(())
    }
    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>> {
        Ok(Vec::new())
    }
    async fn list_files(&self, _branch: &str) -> Result<Vec<String>> {
        Ok(self.files.clone())
    }
    async fn read_file(&self, _branch: &str, path: &str) -> Result<RemoteDocument> {
        Ok(RemoteDocument {
            path: path.to_string(),
            content: "# Mock\n".to_string(),
            revision: self.head.clone(),
        })
    }
    async fn write_files(
        &self,
        _branch: &str,
        _inputs: Vec<WriteFileInput>,
    ) -> Result<Vec<WriteFileResult>> {
        Ok(Vec::new())
    }
    async fn delete_file(&self, _branch: &str, _path: &str, _message: &str) -> Result<()> {
        Ok(())
    }
    async fn get_head(&self, _branch: &str) -> Result<String> {
        Ok(self.head.clone())
    }
}

fn test_workspace() -> Workspace {
    Workspace {
        id: "wk_plan_test".to_string(),
        name: "Test".to_string(),
        mode: WorkspaceMode::ExistingLocalRepo,
        provider: Provider::Github,
        remote_url: None,
        owner: None,
        repo_name: None,
        branch: "main".to_string(),
        local_mode: LocalMode::Sandbox,
        local_root: "/tmp/test".to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: "2026-04-06T00:00:00Z".to_string(),
        updated_at: "2026-04-06T00:00:00Z".to_string(),
    }
}

#[tokio::test]
async fn test_noop_when_nothing_changed() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    // Set checkpoint to match remote head.
    sync_repo::upsert_checkpoint(
        &conn,
        &SyncCheckpoint {
            workspace_id: ws.id.clone(),
            remote_head: "abc123".to_string(),
            last_completed_at: "2026-04-06T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let transport = MockTransport {
        head: "abc123".to_string(),
        files: vec!["notes.md".to_string()],
    };

    let plan = create_sync_plan(&ws, &transport, &conn).await.unwrap();
    assert_eq!(plan.mode, SyncMode::Noop);
    assert!(plan.documents.is_empty());
}

#[tokio::test]
async fn test_pull_only_when_remote_changed() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    sync_repo::upsert_checkpoint(
        &conn,
        &SyncCheckpoint {
            workspace_id: ws.id.clone(),
            remote_head: "old_head".to_string(),
            last_completed_at: "2026-04-06T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let transport = MockTransport {
        head: "new_head".to_string(),
        files: vec!["notes.md".to_string()],
    };

    let plan = create_sync_plan(&ws, &transport, &conn).await.unwrap();
    assert_eq!(plan.mode, SyncMode::PullOnly);
    assert!(!plan.documents.is_empty());
}

#[tokio::test]
async fn test_push_only_when_local_dirty() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    sync_repo::upsert_checkpoint(
        &conn,
        &SyncCheckpoint {
            workspace_id: ws.id.clone(),
            remote_head: "same_head".to_string(),
            last_completed_at: "2026-04-06T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    // Create a dirty local document.
    document_repo::upsert(
        &conn,
        &Document {
            workspace_id: ws.id.clone(),
            path: "notes.md".to_string(),
            local_revision: None,
            remote_revision: None,
            checksum: "abc".to_string(),
            dirty: true,
            deleted: false,
            updated_at: "2026-04-06T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let transport = MockTransport {
        head: "same_head".to_string(),
        files: vec!["notes.md".to_string()],
    };

    let plan = create_sync_plan(&ws, &transport, &conn).await.unwrap();
    assert_eq!(plan.mode, SyncMode::PushOnly);
}

#[tokio::test]
async fn test_pull_then_push_when_both_changed() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    sync_repo::upsert_checkpoint(
        &conn,
        &SyncCheckpoint {
            workspace_id: ws.id.clone(),
            remote_head: "old_head".to_string(),
            last_completed_at: "2026-04-06T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    document_repo::upsert(
        &conn,
        &Document {
            workspace_id: ws.id.clone(),
            path: "notes.md".to_string(),
            local_revision: None,
            remote_revision: None,
            checksum: "abc".to_string(),
            dirty: true,
            deleted: false,
            updated_at: "2026-04-06T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let transport = MockTransport {
        head: "new_head".to_string(),
        files: vec!["notes.md".to_string()],
    };

    let plan = create_sync_plan(&ws, &transport, &conn).await.unwrap();
    assert_eq!(plan.mode, SyncMode::PullThenPush);
}

#[tokio::test]
async fn test_first_sync_is_pull() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    // No checkpoint = first sync.
    let transport = MockTransport {
        head: "initial_head".to_string(),
        files: vec!["notes.md".to_string(), "todo.md".to_string()],
    };

    let plan = create_sync_plan(&ws, &transport, &conn).await.unwrap();
    assert_eq!(plan.mode, SyncMode::PullOnly);
    assert_eq!(plan.documents.len(), 2);
}

#[tokio::test]
async fn test_new_remote_file_detected() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    sync_repo::upsert_checkpoint(
        &conn,
        &SyncCheckpoint {
            workspace_id: ws.id.clone(),
            remote_head: "old_head".to_string(),
            last_completed_at: "2026-04-06T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let transport = MockTransport {
        head: "new_head".to_string(),
        files: vec!["notes.md".to_string(), "new_file.md".to_string()],
    };

    let plan = create_sync_plan(&ws, &transport, &conn).await.unwrap();
    let new_file_plan = plan
        .documents
        .iter()
        .find(|d| d.path == "new_file.md")
        .unwrap();
    assert_eq!(new_file_plan.remote_state, DocumentState::Added);
    assert!(new_file_plan.requires_download);
}
