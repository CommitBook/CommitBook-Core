use commitbook_engine::config::{CommitAgent, CommitMode};

#[tokio::test]
async fn timestamp_mode_does_not_construct_or_probe_providers() {
    let message = super::generate_commit_message_with_chain(
        std::path::Path::new("/unused"),
        &commitbook_engine::git::ChangesSummary {
            modified_files: vec!["notes.md".into()],
            ..Default::default()
        },
        CommitMode::Timestamp,
        CommitAgent::Claude,
        || panic!("Timestamp mode must never construct, probe, or invoke providers"),
    )
    .await;
    assert_eq!(message.len(), "Writing YYYY-MM-DD HH:MM:SS".len());
    chrono::NaiveDateTime::parse_from_str(
        message.strip_prefix("Writing ").unwrap(),
        "%Y-%m-%d %H:%M:%S",
    )
    .unwrap();
}

#[test]
fn contended_lock_logs_a_skipped_sync() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".CommitBook/local")).unwrap();

    super::log_skipped_sync(tmp.path());

    let logs = std::fs::read_dir(tmp.path().join(".CommitBook/local/logs"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    assert!(
        logs.contains("Sync skipped: another operation is running"),
        "{logs}"
    );
}
