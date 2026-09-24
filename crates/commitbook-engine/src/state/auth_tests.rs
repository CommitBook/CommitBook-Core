use super::*;

#[test]
fn test_default_auth() {
    let auth = AuthConfig::default();
    assert!(!auth.has_token());
    assert!(auth.token().is_none());
    assert!(auth.provider().is_none());
}

#[test]
fn test_load_missing_returns_default() {
    let tmp = tempfile::tempdir().unwrap();
    let auth = AuthConfig::load(tmp.path()).unwrap();
    assert!(!auth.has_token());
}

#[test]
fn test_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let auth = AuthConfig {
        auth: AuthEntry {
            provider: Some("github".to_string()),
            token: Some("ghp_test123".to_string()),
        },
    };
    auth.save(tmp.path()).unwrap();

    let loaded = AuthConfig::load(tmp.path()).unwrap();
    assert!(loaded.has_token());
    assert_eq!(loaded.token(), Some("ghp_test123"));
    assert_eq!(loaded.provider(), Some("github"));
}

#[test]
fn test_save_creates_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("local").join("auth.toml");
    assert!(!path.exists());

    AuthConfig::default().save(tmp.path()).unwrap();
    assert!(path.exists());
}

#[test]
fn test_has_token_false_when_none() {
    let auth = AuthConfig {
        auth: AuthEntry {
            provider: Some("github".to_string()),
            token: None,
        },
    };
    assert!(!auth.has_token());
}

#[test]
fn test_has_token_true_when_set() {
    let auth = AuthConfig {
        auth: AuthEntry {
            provider: None,
            token: Some("tok".to_string()),
        },
    };
    assert!(auth.has_token());
    assert_eq!(auth.token(), Some("tok"));
}

#[cfg(unix)]
#[test]
fn test_save_sets_restrictive_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let auth = AuthConfig {
        auth: AuthEntry {
            provider: Some("github".to_string()),
            token: Some("secret".to_string()),
        },
    };
    auth.save(tmp.path()).unwrap();

    let path = tmp.path().join("local").join("auth.toml");
    let perms = std::fs::metadata(&path).unwrap().permissions();
    assert_eq!(perms.mode() & 0o777, 0o600);
}

#[test]
fn test_clear_removes_saved_token() {
    let tmp = tempfile::tempdir().unwrap();
    let auth = AuthConfig {
        auth: AuthEntry {
            provider: Some("github".into()),
            token: Some("t".into()),
        },
    };
    auth.save(tmp.path()).unwrap();
    assert!(AuthConfig::clear(tmp.path()).unwrap());
    assert!(!AuthConfig::load(tmp.path()).unwrap().has_token());
}

#[test]
fn test_clear_missing_returns_false() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!AuthConfig::clear(tmp.path()).unwrap());
}
