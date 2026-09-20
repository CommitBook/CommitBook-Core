use super::*;

#[test]
fn timestamp_message_has_only_static_text_and_local_datetime() {
    for summary in [
        ChangesSummary::default(),
        ChangesSummary {
            new_files: vec!["test.md".to_string()],
            modified_files: vec!["notes.md".to_string()],
            deleted_files: vec!["old.md".to_string()],
        },
    ] {
        let before = chrono::Local::now().naive_local();
        let msg = generate_timestamp_message(&summary);
        let after = chrono::Local::now().naive_local();
        assert_eq!(msg.len(), "Writing YYYY-MM-DD HH:MM:SS".len());
        let timestamp = msg.strip_prefix("Writing ").unwrap();
        let parsed = chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S")
            .expect("valid local datetime with no change counts");
        assert!(parsed >= before - chrono::Duration::seconds(1));
        assert!(parsed <= after);
    }
}
