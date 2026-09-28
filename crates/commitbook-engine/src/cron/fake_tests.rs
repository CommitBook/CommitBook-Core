use super::*;

#[test]
fn running_fake_reports_loaded_and_records_calls() {
    let fake = FakeScheduler::running("0 * * * *");
    let repo = Path::new("/tmp/repo");
    assert!(fake.is_loaded(repo));
    assert_eq!(fake.installed_schedule().as_deref(), Some("0 * * * *"));
    fake.uninstall(repo).unwrap();
    assert!(!fake.is_loaded(repo));
    assert_eq!(
        fake.calls(),
        vec![FakeCall::IsLoaded, FakeCall::Uninstall, FakeCall::IsLoaded]
    );
}

#[test]
fn queued_install_failures_are_consumed_in_order() {
    let fake = FakeScheduler::stopped();
    let repo = Path::new("/tmp/repo");
    fake.fail_next_install("first");
    fake.fail_next_install("second");
    let bin = Path::new("commitbook");
    assert_eq!(
        fake.install(repo, "*/5 * * * *", bin)
            .unwrap_err()
            .to_string(),
        "first"
    );
    assert_eq!(
        fake.install(repo, "*/5 * * * *", bin)
            .unwrap_err()
            .to_string(),
        "second"
    );
    assert!(fake.installed_schedule().is_none());
    fake.install(repo, "*/5 * * * *", bin).unwrap();
    assert_eq!(fake.installed_schedule().as_deref(), Some("*/5 * * * *"));
    assert_eq!(fake.install_count(), 3);
}
