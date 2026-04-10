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
fn test_create_transport_with_pat_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(&cb_dir).unwrap();

    // Write auth with a token.
    let auth = AuthConfig {
        auth: AuthEntry {
            provider: Some("github".to_string()),
            token: Some("ghp_test".to_string()),
        },
    };
    auth.save(&cb_dir).unwrap();

    let config = LocalConfig::new("0 * * * *");
    let result = create_transport(&cb_dir, tmp.path(), &config);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("not yet implemented"));
}
