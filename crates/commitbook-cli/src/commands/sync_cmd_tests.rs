use super::commit_provider_keys;

#[test]
fn test_provider_keys_ai_enabled_uses_full_chain() {
    let keys = commit_provider_keys(true);
    assert_eq!(
        keys,
        vec![
            "gh-copilot".to_string(),
            "claude-cli".to_string(),
            "codex-cli".to_string(),
            "fallback".to_string(),
        ]
    );
}

#[test]
fn test_provider_keys_ai_disabled_uses_fallback_only() {
    let keys = commit_provider_keys(false);
    assert_eq!(keys, vec!["fallback".to_string()]);
}
