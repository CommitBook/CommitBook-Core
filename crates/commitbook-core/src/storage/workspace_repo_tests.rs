use crate::domain::workspace::*;
use crate::storage::db;
use crate::storage::workspace_repo;

fn test_workspace() -> Workspace {
    Workspace {
        id: Workspace::new_id(),
        name: "Test Notes".to_string(),
        mode: WorkspaceMode::ExistingLocalRepo,
        provider: Provider::Github,
        remote_url: Some("git@github.com:user/notes.git".to_string()),
        owner: Some("user".to_string()),
        repo_name: Some("notes".to_string()),
        branch: "main".to_string(),
        local_mode: LocalMode::Folder,
        local_root: "/tmp/notes".to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: "2026-04-06T20:00:00Z".to_string(),
        updated_at: "2026-04-06T20:00:00Z".to_string(),
    }
}

#[test]
fn test_insert_and_get() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();

    workspace_repo::insert(&conn, &ws).unwrap();
    let loaded = workspace_repo::get(&conn, &ws.id).unwrap().unwrap();

    assert_eq!(loaded.id, ws.id);
    assert_eq!(loaded.name, "Test Notes");
    assert_eq!(loaded.mode, WorkspaceMode::ExistingLocalRepo);
    assert_eq!(loaded.provider, Provider::Github);
    assert_eq!(loaded.branch, "main");
    assert_eq!(loaded.sync_interval_seconds, 300);
    assert!(loaded.auto_sync);
}

#[test]
fn test_get_nonexistent() {
    let conn = db::open_in_memory().unwrap();
    let result = workspace_repo::get(&conn, "nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_list_empty() {
    let conn = db::open_in_memory().unwrap();
    let list = workspace_repo::list(&conn).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_list_multiple() {
    let conn = db::open_in_memory().unwrap();

    let mut ws1 = test_workspace();
    ws1.name = "First".to_string();
    workspace_repo::insert(&conn, &ws1).unwrap();

    let mut ws2 = test_workspace();
    ws2.id = Workspace::new_id();
    ws2.name = "Second".to_string();
    ws2.local_root = "/tmp/second".to_string();
    workspace_repo::insert(&conn, &ws2).unwrap();

    let list = workspace_repo::list(&conn).unwrap();
    assert_eq!(list.len(), 2);
}

#[test]
fn test_update() {
    let conn = db::open_in_memory().unwrap();
    let mut ws = test_workspace();
    workspace_repo::insert(&conn, &ws).unwrap();

    ws.name = "Updated Name".to_string();
    ws.sync_interval_seconds = 600;
    ws.updated_at = "2026-04-07T10:00:00Z".to_string();
    workspace_repo::update(&conn, &ws).unwrap();

    let loaded = workspace_repo::get(&conn, &ws.id).unwrap().unwrap();
    assert_eq!(loaded.name, "Updated Name");
    assert_eq!(loaded.sync_interval_seconds, 600);
}

#[test]
fn test_delete() {
    let conn = db::open_in_memory().unwrap();
    let ws = test_workspace();
    workspace_repo::insert(&conn, &ws).unwrap();

    workspace_repo::delete(&conn, &ws.id).unwrap();
    let loaded = workspace_repo::get(&conn, &ws.id).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_all_modes_roundtrip() {
    let conn = db::open_in_memory().unwrap();

    for mode in &[
        WorkspaceMode::GithubApp,
        WorkspaceMode::Pat,
        WorkspaceMode::Ssh,
        WorkspaceMode::ExistingLocalRepo,
    ] {
        let mut ws = test_workspace();
        ws.id = Workspace::new_id();
        ws.mode = mode.clone();
        ws.local_root = format!("/tmp/{}", ws.id);
        workspace_repo::insert(&conn, &ws).unwrap();
        let loaded = workspace_repo::get(&conn, &ws.id).unwrap().unwrap();
        assert_eq!(loaded.mode, *mode);
    }
}

#[test]
fn test_all_providers_roundtrip() {
    let conn = db::open_in_memory().unwrap();

    for provider in &[
        Provider::Github,
        Provider::Gitlab,
        Provider::Codeberg,
        Provider::GenericGit,
    ] {
        let mut ws = test_workspace();
        ws.id = Workspace::new_id();
        ws.provider = provider.clone();
        ws.local_root = format!("/tmp/{}", ws.id);
        workspace_repo::insert(&conn, &ws).unwrap();
        let loaded = workspace_repo::get(&conn, &ws.id).unwrap().unwrap();
        assert_eq!(loaded.provider, *provider);
    }
}
