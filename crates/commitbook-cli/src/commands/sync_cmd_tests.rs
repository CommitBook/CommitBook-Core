use super::*;

#[test]
fn test_create_transport_errors_without_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local")).unwrap();
    // Initialize a bare git dir so get_remote_url can attempt Repository::open,
    // but with no remote configured.
    git2::Repository::init(tmp.path()).unwrap();

    let config = LocalConfig::new("0 * * * *");
    let err = create_transport(&cb_dir, tmp.path(), &config).err().expect(
        "expected create_transport to error when configured remote is missing",
    );
    let msg = err.to_string();
    assert!(msg.contains("not found"), "unexpected error: {msg}");
}

#[test]
fn test_create_transport_with_configured_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local")).unwrap();

    // Real git repo with a remote named after whatever the user picked.
    let repo = git2::Repository::init(tmp.path()).unwrap();
    repo.remote("upstream", "https://example.com/repo.git").unwrap();

    let mut config = LocalConfig::new("0 * * * *");
    config.git.remote = "upstream".to_string();

    let result = create_transport(&cb_dir, tmp.path(), &config);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}
