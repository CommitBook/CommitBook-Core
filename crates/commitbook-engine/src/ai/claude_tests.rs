use super::*;

#[test]
fn test_claude_name() {
    let provider = ClaudeProvider;
    assert_eq!(provider.name(), "Claude Code");
}

#[test]
fn test_claude_key() {
    let provider = ClaudeProvider;
    assert_eq!(provider.key(), "claude-cli");
}

#[test]
fn test_claude_is_available_returns_bool() {
    let provider = ClaudeProvider;
    // Should not panic regardless of whether claude CLI is installed
    let _available: bool = provider.is_available();
}
