use super::*;
use commitbook_core::domain::workspace::*;

fn test_workspace(mode: WorkspaceMode, remote_url: Option<String>, local_root: &str) -> Workspace {
    Workspace {
        id: "wk_test123".to_string(),
        name: "test".to_string(),
        mode,
        provider: Provider::GenericGit,
        remote_url,
        owner: None,
        repo_name: None,
        branch: "main".to_string(),
        local_mode: LocalMode::Folder,
        local_root: local_root.to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    }
}

#[test]
fn test_create_transport_existing_local_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = test_workspace(
        WorkspaceMode::ExistingLocalRepo,
        None,
        &tmp.path().to_string_lossy(),
    );
    assert!(create_transport(&ws).is_ok());
}

#[test]
fn test_create_transport_ssh_with_url() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = test_workspace(
        WorkspaceMode::Ssh,
        Some("git@github.com:user/repo.git".to_string()),
        &tmp.path().to_string_lossy(),
    );
    assert!(create_transport(&ws).is_ok());
}

#[test]
fn test_create_transport_ssh_missing_url() {
    let ws = test_workspace(WorkspaceMode::Ssh, None, "/tmp/test");
    match create_transport(&ws) {
        Err(e) => assert!(
            e.to_string().contains("missing remote URL"),
            "got: {}",
            e
        ),
        Ok(_) => panic!("expected error for SSH without URL"),
    }
}

#[test]
fn test_create_transport_pat_errors() {
    let ws = test_workspace(WorkspaceMode::Pat, None, "/tmp/test");
    match create_transport(&ws) {
        Err(e) => assert!(
            e.to_string().contains("authentication"),
            "got: {}",
            e
        ),
        Ok(_) => panic!("expected error for PAT mode"),
    }
}

#[test]
fn test_create_transport_github_app_errors() {
    let ws = test_workspace(WorkspaceMode::GithubApp, None, "/tmp/test");
    match create_transport(&ws) {
        Err(e) => assert!(
            e.to_string().contains("backend session"),
            "got: {}",
            e
        ),
        Ok(_) => panic!("expected error for GithubApp mode"),
    }
}
