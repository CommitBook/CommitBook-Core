use super::*;

#[test]
fn test_fallback_no_changes() {
    let summary = ChangesSummary::default();
    let msg = generate_timestamp_message(&summary);
    assert!(msg.starts_with("Writing "));
    assert!(!msg.contains('('));
}

#[test]
fn test_fallback_with_changes() {
    let summary = ChangesSummary {
        new_files: vec!["test.md".to_string()],
        modified_files: vec!["notes.md".to_string()],
        deleted_files: vec![],
    };
    let msg = generate_timestamp_message(&summary);
    assert!(msg.starts_with("Writing "));
    assert!(msg.contains("1 new"));
    assert!(msg.contains("1 modified"));
}
