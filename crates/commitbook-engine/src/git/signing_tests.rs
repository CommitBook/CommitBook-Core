#![cfg(not(any(target_os = "ios", target_os = "android")))]

use super::*;

#[test]
fn resolve_ssh_key_file_writes_literal_key_to_temp_file() {
    let literal = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAtest user@host";
    let (path, tmp) = resolve_ssh_key_file(literal).unwrap();

    // A literal key must be materialized to a real temp file, not passed as-is.
    assert!(tmp.is_some(), "literal key should produce a temp file");
    assert_ne!(path, literal);
    assert!(std::path::Path::new(&path).exists());
    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents.trim(), literal);
}

#[test]
fn resolve_ssh_key_file_strips_key_prefix() {
    let (path, tmp) = resolve_ssh_key_file("key::ssh-ed25519 AAAAtest").unwrap();
    assert!(tmp.is_some());
    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents.trim(), "ssh-ed25519 AAAAtest");
}

#[test]
fn resolve_ssh_key_file_treats_plain_value_as_path() {
    let (path, tmp) = resolve_ssh_key_file("/home/me/.ssh/id_ed25519").unwrap();
    assert!(
        tmp.is_none(),
        "a path value should not be copied to a temp file"
    );
    assert_eq!(path, "/home/me/.ssh/id_ed25519");
}

#[test]
fn gpg_arguments_allow_default_key_selection() {
    let arguments = gpg_arguments(None);
    assert_eq!(arguments, ["--sign", "--armor", "--detach-sign"]);
    assert!(!arguments.iter().any(|argument| argument == "--local-user"));
}

#[test]
fn gpg_arguments_include_explicit_key_when_configured() {
    let arguments = gpg_arguments(Some("ABC123"));
    assert_eq!(
        arguments,
        [
            "--sign",
            "--armor",
            "--detach-sign",
            "--local-user",
            "ABC123"
        ]
    );
}
