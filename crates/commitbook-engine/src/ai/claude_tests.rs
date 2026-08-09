use super::*;
use crate::ai::CommitMessageProvider;
use crate::ai::ConflictResolver;

#[test]
fn test_claude_commit_provider_name() {
    let provider = ClaudeProvider;
    assert_eq!(CommitMessageProvider::name(&provider), "Claude Code");
}

#[test]
fn test_claude_commit_provider_key() {
    let provider = ClaudeProvider;
    assert_eq!(CommitMessageProvider::key(&provider), "claude-cli");
}

#[test]
fn test_claude_resolver_key() {
    let provider = ClaudeProvider;
    assert_eq!(ConflictResolver::key(&provider), "claude");
}

#[test]
fn test_claude_is_available_returns_bool() {
    let provider = ClaudeProvider;
    // Should not panic regardless of whether claude CLI is installed
    let _commit: bool = CommitMessageProvider::is_available(&provider);
    let _resolve: bool = ConflictResolver::is_available(&provider);
}
