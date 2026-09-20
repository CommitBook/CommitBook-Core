use super::*;

#[cfg(unix)]
#[test]
fn system_credentials_uses_repository_local_helper() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let mut config = repo.config().unwrap();
    config
        .set_str(
            "credential.helper",
            "!f() { printf 'username=repo-user\\npassword=repo-password\\n'; }; f",
        )
        .unwrap();

    let credential = SystemCredentials
        .provide(
            &config,
            "https://example.invalid/repository.git",
            None,
            CredentialType::USER_PASS_PLAINTEXT,
        )
        .unwrap();

    assert_eq!(
        credential.credtype(),
        CredentialType::USER_PASS_PLAINTEXT.bits()
    );
}

#[test]
fn token_credentials_ignore_repository_configuration() {
    let config = Config::new().unwrap();
    let credential = TokenCredentials::new("token")
        .provide(
            &config,
            "https://example.invalid/repository.git",
            None,
            CredentialType::USER_PASS_PLAINTEXT,
        )
        .unwrap();
    assert_eq!(
        credential.credtype(),
        CredentialType::USER_PASS_PLAINTEXT.bits()
    );
}
