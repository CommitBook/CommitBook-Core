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

#[tokio::test]
async fn disabled_ai_does_not_construct_or_probe_providers() {
    let message = super::generate_commit_message_with_chain(
        std::path::Path::new("/unused"),
        &commitbook_engine::git::ChangesSummary {
            modified_files: vec!["notes.md".into()],
            ..Default::default()
        },
        false,
        || panic!("Disabled AI must never construct, probe, or invoke providers"),
    )
    .await;
    assert_eq!(message.len(), "Writing YYYY-MM-DD HH:MM:SS".len());
    chrono::NaiveDateTime::parse_from_str(
        message.strip_prefix("Writing ").unwrap(),
        "%Y-%m-%d %H:%M:%S",
    )
    .unwrap();
}
