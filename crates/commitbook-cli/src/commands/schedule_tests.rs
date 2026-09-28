use super::*;
use commitbook_engine::config::LocalConfig;
use commitbook_engine::cron::FakeScheduler;
use std::path::PathBuf;

fn init_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "remote",
            "add",
            "origin",
            "https://example.invalid/notes.git",
        ],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(tmp.path())
            .status()
            .expect("git should run");
        assert!(status.success(), "git {args:?} failed");
    }
    LocalConfig::init(tmp.path(), &LocalConfig::new("notes", "main", "origin")).unwrap();
    tmp
}

#[test]
fn schedule_command_normalizes_presets_and_intervals() {
    let tmp = init_repo();
    let fake = FakeScheduler::running("0 * * * *");
    let context = SchedulerContext::new(&fake, PathBuf::from("commitbook"));

    let outcome = run_with(tmp.path(), "every-5m", &context).unwrap();
    assert_eq!(outcome.config.sync.schedule, "5m");
    assert!(outcome.scheduler_reinstalled);
    assert_eq!(fake.installed_schedule().as_deref(), Some("5m"));

    let outcome = run_with(tmp.path(), "0 */2 * * *", &context).unwrap();
    assert_eq!(outcome.config.sync.schedule, "2h");
    assert_eq!(LocalConfig::load(tmp.path()).unwrap().sync.schedule, "2h");

    assert!(run_with(tmp.path(), "not a cron", &context).is_err());
    assert_eq!(LocalConfig::load(tmp.path()).unwrap().sync.schedule, "2h");
}

#[test]
fn schedule_command_does_not_reinstall_a_stopped_scheduler() {
    let tmp = init_repo();
    let fake = FakeScheduler::stopped();
    let context = SchedulerContext::new(&fake, PathBuf::from("commitbook"));
    let outcome = run_with(tmp.path(), "daily", &context).unwrap();
    assert!(!outcome.scheduler_reinstalled);
    assert_eq!(fake.install_count(), 0);
}
