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

#[tokio::test]
async fn unloadable_config_is_logged_and_recorded_as_the_last_error() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .status()
        .unwrap()
        .success());
    std::fs::create_dir_all(tmp.path().join(".CommitBook/local")).unwrap();
    std::fs::write(tmp.path().join(".CommitBook/config.toml"), "not = [valid").unwrap();

    let error = super::run_sync(tmp.path()).await.unwrap_err();
    assert!(format!("{error:#}").contains("config"), "{error:#}");

    let logs = std::fs::read_dir(tmp.path().join(".CommitBook/local/logs"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    assert!(
        logs.contains("cannot load .CommitBook/config.toml"),
        "{logs}"
    );

    let state =
        commitbook_engine::state::sync_state::SyncState::load(&tmp.path().join(".CommitBook"))
            .unwrap();
    assert_eq!(state.last_error_stage.as_deref(), Some("config"));
    assert!(state.last_attempt_at.is_some());
    assert!(state
        .last_error
        .unwrap()
        .contains("cannot load .CommitBook/config.toml"));
}
