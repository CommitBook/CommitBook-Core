use super::*;
use commitbook_core::state::auth::{AuthConfig, AuthEntry};

#[test]
fn test_create_transport_no_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(&cb_dir).unwrap();
    let config = LocalConfig::new("0 * * * *");

    let result = create_transport(&cb_dir, tmp.path(), &config);
    assert!(result.is_ok());
}

#[test]
fn test_create_transport_with_pat_falls_back_to_local() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(&cb_dir).unwrap();

    // Write auth with a token — transport should succeed, falling back to local.
    let auth = AuthConfig {
        auth: AuthEntry {
            provider: Some("github".to_string()),
            token: Some("ghp_test".to_string()),
        },
    };
    auth.save(&cb_dir).unwrap();

    let config = LocalConfig::new("0 * * * *");
    let result = create_transport(&cb_dir, tmp.path(), &config);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}
