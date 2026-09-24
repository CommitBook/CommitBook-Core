use super::*;
use crate::cron::FakeScheduler;
use crate::state::RepoLockContended;
use std::fs;

fn init_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    repo.remote("origin", "https://example.invalid/notes.git")
        .unwrap();
    LocalConfig::init(tmp.path(), "0 * * * *").unwrap();
    tmp
}

fn context(adapter: &FakeScheduler) -> SchedulerContext<'_> {
    SchedulerContext::new(adapter, PathBuf::from("/usr/local/bin/commitbook"))
}

fn schedule_update(schedule: &str) -> SettingsUpdate {
    SettingsUpdate {
        schedule: Some(schedule.to_string()),
        ..Default::default()
    }
}

#[test]
fn normalize_schedule_accepts_presets_intervals_and_cron() {
    assert_eq!(normalize_schedule("hourly").unwrap(), "0 * * * *");
    assert_eq!(normalize_schedule("5m").unwrap(), "*/5 * * * *");
    assert_eq!(
        normalize_schedule(" */15 * * * * ").unwrap(),
        "*/15 * * * *"
    );
    assert!(normalize_schedule("not a cron").is_err());
    assert!(normalize_schedule("").is_err());
}

#[test]
fn update_saves_and_skips_scheduler_when_stopped() {
    let tmp = init_repo();
    let fake = FakeScheduler::stopped();
    let outcome =
        update_settings(tmp.path(), &schedule_update("every-5m"), &context(&fake)).unwrap();
    assert!(outcome.schedule_changed);
    assert!(!outcome.scheduler_reinstalled);
    assert_eq!(outcome.config.schedule, "*/5 * * * *");
    assert_eq!(
        LocalConfig::load(tmp.path()).unwrap().schedule,
        "*/5 * * * *"
    );
    assert_eq!(fake.install_count(), 0);
}

#[test]
fn update_reinstalls_active_scheduler_with_new_schedule() {
    let tmp = init_repo();
    let fake = FakeScheduler::running("0 * * * *");
    let outcome = update_settings(tmp.path(), &schedule_update("5m"), &context(&fake)).unwrap();
    assert!(outcome.scheduler_reinstalled);
    assert_eq!(fake.installed_schedule().as_deref(), Some("*/5 * * * *"));
    assert!(fake
        .calls()
        .contains(&crate::cron::fake::FakeCall::Install {
            schedule: "*/5 * * * *".into(),
            binary: PathBuf::from("/usr/local/bin/commitbook"),
        }));
}

#[test]
fn install_failure_restores_old_config_and_old_job() {
    let tmp = init_repo();
    let fake = FakeScheduler::running("0 * * * *");
    fake.fail_next_install("launchctl load failed");
    let error = update_settings(tmp.path(), &schedule_update("5m"), &context(&fake)).unwrap_err();
    let details = error
        .downcast_ref::<SettingsUpdateError>()
        .expect("settings error");
    assert!(details
        .original
        .to_string()
        .contains("launchctl load failed"));
    assert!(details.rollback.is_none());
    assert!(error.to_string().contains("were restored"));
    assert_eq!(LocalConfig::load(tmp.path()).unwrap().schedule, "0 * * * *");
    assert_eq!(fake.installed_schedule().as_deref(), Some("0 * * * *"));
    assert_eq!(fake.install_count(), 2);
}

#[test]
fn install_and_rollback_failures_are_both_reported() {
    let tmp = init_repo();
    let fake = FakeScheduler::running("0 * * * *");
    fake.fail_next_install("new job rejected");
    fake.fail_next_install("old job rejected");
    let error = update_settings(tmp.path(), &schedule_update("5m"), &context(&fake)).unwrap_err();
    let details = error
        .downcast_ref::<SettingsUpdateError>()
        .expect("settings error");
    assert!(details.original.to_string().contains("new job rejected"));
    let rollback = details.rollback.as_ref().expect("rollback failure");
    assert!(format!("{rollback:#}").contains("old job rejected"));
    let message = error.to_string();
    assert!(message.contains("new job rejected"), "{message}");
    assert!(message.contains("old job rejected"), "{message}");
    assert!(!message.contains("were restored"), "{message}");
    // The configuration itself was rolled back even though the job was not.
    assert_eq!(LocalConfig::load(tmp.path()).unwrap().schedule, "0 * * * *");
}

#[test]
fn concurrent_mutation_is_rejected() {
    let tmp = init_repo();
    let _held = RepoLock::acquire(tmp.path()).unwrap();
    let fake = FakeScheduler::stopped();
    let error = update_settings(tmp.path(), &schedule_update("5m"), &context(&fake)).unwrap_err();
    assert!(error.downcast_ref::<RepoLockContended>().is_some());
    assert_eq!(LocalConfig::load(tmp.path()).unwrap().schedule, "0 * * * *");
}

#[test]
fn unchanged_schedule_does_not_touch_scheduler() {
    let tmp = init_repo();
    let fake = FakeScheduler::running("0 * * * *");
    let before = fs::read(LocalConfig::config_path(tmp.path())).unwrap();
    let outcome = update_settings(tmp.path(), &schedule_update("hourly"), &context(&fake)).unwrap();
    assert!(!outcome.schedule_changed);
    assert!(!outcome.scheduler_reinstalled);
    assert_eq!(fake.install_count(), 0);
    assert_eq!(
        fs::read(LocalConfig::config_path(tmp.path())).unwrap(),
        before
    );

    let update = SettingsUpdate {
        auto_push: Some(false),
        ..Default::default()
    };
    let outcome = update_settings(tmp.path(), &update, &context(&fake)).unwrap();
    assert!(!outcome.schedule_changed);
    assert!(!outcome.config.git.auto_push);
    assert_eq!(fake.install_count(), 0);
}

#[test]
fn invalid_branch_is_rejected_before_save() {
    let tmp = init_repo();
    let fake = FakeScheduler::stopped();
    let before = fs::read(LocalConfig::config_path(tmp.path())).unwrap();
    for branch in ["", "has space", "bad..name", "-leading"] {
        let update = SettingsUpdate {
            branch: Some(branch.to_string()),
            auto_push: Some(false),
            ..Default::default()
        };
        assert!(
            update_settings(tmp.path(), &update, &context(&fake)).is_err(),
            "branch `{branch}` should be rejected"
        );
    }
    assert_eq!(
        fs::read(LocalConfig::config_path(tmp.path())).unwrap(),
        before
    );

    let update = SettingsUpdate {
        branch: Some("notes/main".to_string()),
        ..Default::default()
    };
    let outcome = update_settings(tmp.path(), &update, &context(&fake)).unwrap();
    assert_eq!(outcome.config.git.branch, "notes/main");
}

#[test]
fn start_and_stop_go_through_the_adapter() {
    let tmp = init_repo();
    let fake = FakeScheduler::stopped();
    start_scheduler(tmp.path(), &context(&fake)).unwrap();
    assert_eq!(fake.installed_schedule().as_deref(), Some("0 * * * *"));
    stop_scheduler(tmp.path(), &context(&fake)).unwrap();
    assert!(fake.installed_schedule().is_none());

    let _held = RepoLock::acquire(tmp.path()).unwrap();
    let error = start_scheduler(tmp.path(), &context(&fake)).unwrap_err();
    assert!(error.downcast_ref::<RepoLockContended>().is_some());
}

#[test]
fn auto_merge_appends_update_is_saved() {
    let tmp = init_repo();
    let fake = FakeScheduler::stopped();
    let update = SettingsUpdate {
        auto_merge_appends: Some(false),
        ..Default::default()
    };
    let outcome = update_settings(tmp.path(), &update, &context(&fake)).unwrap();
    assert!(!outcome.config.conflict.auto_merge_appends);
    assert!(
        !LocalConfig::load(tmp.path())
            .unwrap()
            .conflict
            .auto_merge_appends
    );
}
