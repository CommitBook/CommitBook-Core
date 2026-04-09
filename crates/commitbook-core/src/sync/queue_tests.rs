use super::*;
use crate::domain::sync_plan::SyncJobType;
use crate::storage::db;

#[test]
fn test_enqueue_new_job() {
    let conn = db::open_in_memory().unwrap();

    // Create a workspace first.
    let ws = crate::domain::workspace::Workspace {
        id: "wk_test".to_string(),
        name: "Test".to_string(),
        mode: crate::domain::workspace::WorkspaceMode::ExistingLocalRepo,
        provider: crate::domain::workspace::Provider::Github,
        remote_url: None,
        owner: None,
        repo_name: None,
        branch: "main".to_string(),
        local_mode: crate::domain::workspace::LocalMode::Sandbox,
        local_root: "/tmp/test".to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: "2026-04-06T00:00:00Z".to_string(),
        updated_at: "2026-04-06T00:00:00Z".to_string(),
    };
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    let job_id = enqueue(&conn, "wk_test", SyncJobType::SyncNow).unwrap();
    assert!(job_id.is_some());

    let job = dequeue(&conn, "wk_test").unwrap();
    assert!(job.is_some());
    let job = job.unwrap();
    assert_eq!(job.workspace_id, "wk_test");
    assert_eq!(job.status, crate::domain::sync_plan::SyncJobStatus::Pending);
}

#[test]
fn test_no_duplicate_active_jobs() {
    let conn = db::open_in_memory().unwrap();

    let ws = crate::domain::workspace::Workspace {
        id: "wk_test2".to_string(),
        name: "Test".to_string(),
        mode: crate::domain::workspace::WorkspaceMode::ExistingLocalRepo,
        provider: crate::domain::workspace::Provider::Github,
        remote_url: None,
        owner: None,
        repo_name: None,
        branch: "main".to_string(),
        local_mode: crate::domain::workspace::LocalMode::Sandbox,
        local_root: "/tmp/test2".to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: "2026-04-06T00:00:00Z".to_string(),
        updated_at: "2026-04-06T00:00:00Z".to_string(),
    };
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    let first = enqueue(&conn, "wk_test2", SyncJobType::SyncNow).unwrap();
    assert!(first.is_some());

    let second = enqueue(&conn, "wk_test2", SyncJobType::SyncNow).unwrap();
    assert!(second.is_none()); // Already has active job.
}

#[test]
fn test_mark_completed() {
    let conn = db::open_in_memory().unwrap();

    let ws = crate::domain::workspace::Workspace {
        id: "wk_test3".to_string(),
        name: "Test".to_string(),
        mode: crate::domain::workspace::WorkspaceMode::ExistingLocalRepo,
        provider: crate::domain::workspace::Provider::Github,
        remote_url: None,
        owner: None,
        repo_name: None,
        branch: "main".to_string(),
        local_mode: crate::domain::workspace::LocalMode::Sandbox,
        local_root: "/tmp/test3".to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: "2026-04-06T00:00:00Z".to_string(),
        updated_at: "2026-04-06T00:00:00Z".to_string(),
    };
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    let job_id = enqueue(&conn, "wk_test3", SyncJobType::SyncNow)
        .unwrap()
        .unwrap();
    mark_running(&conn, &job_id).unwrap();
    mark_completed(&conn, &job_id).unwrap();

    // Should be able to enqueue again after completion.
    let new_job = enqueue(&conn, "wk_test3", SyncJobType::ScheduledPull).unwrap();
    assert!(new_job.is_some());
}

#[test]
fn test_mark_failed() {
    let conn = db::open_in_memory().unwrap();

    let ws = crate::domain::workspace::Workspace {
        id: "wk_test4".to_string(),
        name: "Test".to_string(),
        mode: crate::domain::workspace::WorkspaceMode::ExistingLocalRepo,
        provider: crate::domain::workspace::Provider::Github,
        remote_url: None,
        owner: None,
        repo_name: None,
        branch: "main".to_string(),
        local_mode: crate::domain::workspace::LocalMode::Sandbox,
        local_root: "/tmp/test4".to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: "2026-04-06T00:00:00Z".to_string(),
        updated_at: "2026-04-06T00:00:00Z".to_string(),
    };
    crate::storage::workspace_repo::insert(&conn, &ws).unwrap();

    let job_id = enqueue(&conn, "wk_test4", SyncJobType::SyncNow)
        .unwrap()
        .unwrap();
    mark_running(&conn, &job_id).unwrap();
    mark_failed(&conn, &job_id, "Network timeout").unwrap();

    // After failure, should be able to enqueue new job.
    let new_job = enqueue(&conn, "wk_test4", SyncJobType::SyncNow).unwrap();
    assert!(new_job.is_some());
}
