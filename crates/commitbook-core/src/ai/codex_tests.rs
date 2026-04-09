use super::*;

#[test]
fn test_codex_name() {
    let provider = CodexProvider;
    assert_eq!(provider.name(), "Codex");
}

#[test]
fn test_codex_key() {
    let provider = CodexProvider;
    assert_eq!(provider.key(), "codex-cli");
}

#[test]
fn test_codex_is_available_returns_bool() {
    let provider = CodexProvider;
    // Should not panic regardless of whether codex CLI is installed
    let _available: bool = provider.is_available();
}
