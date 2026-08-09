use super::*;
use crate::ai::CommitMessageProvider;
use crate::ai::ConflictResolver;

#[test]
fn test_codex_commit_provider_name() {
    let provider = CodexProvider;
    assert_eq!(CommitMessageProvider::name(&provider), "Codex");
}

#[test]
fn test_codex_commit_provider_key() {
    let provider = CodexProvider;
    assert_eq!(CommitMessageProvider::key(&provider), "codex-cli");
}

#[test]
fn test_codex_resolver_key() {
    let provider = CodexProvider;
    assert_eq!(ConflictResolver::key(&provider), "codex");
}

#[test]
fn test_codex_is_available_returns_bool() {
    let provider = CodexProvider;
    let _commit: bool = CommitMessageProvider::is_available(&provider);
    let _resolve: bool = ConflictResolver::is_available(&provider);
}
