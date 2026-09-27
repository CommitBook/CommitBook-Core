use super::*;
use commitbook_engine::config::LocalConfig;

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success());
}

/// An initialized repo whose only problem, if any, is what the test adds.
fn healthy_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    git(
        tmp.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/notes.git",
        ],
    );
    LocalConfig::init(tmp.path(), &LocalConfig::new("notes", "main", "origin")).unwrap();
    tmp
}

#[test]
fn doctor_passes_a_healthy_repository() {
    let tmp = healthy_repo();
    let cb_dir = LocalConfig::commitbook_dir(tmp.path());
    run(&cb_dir, tmp.path(), false, false).unwrap();
}

#[test]
fn doctor_fails_when_config_does_not_parse() {
    let tmp = healthy_repo();
    let cb_dir = LocalConfig::commitbook_dir(tmp.path());
    std::fs::write(
        LocalConfig::config_path(tmp.path()),
        "schedule = every-5m\n",
    )
    .unwrap();
    assert!(run(&cb_dir, tmp.path(), false, false).is_err());
}

#[test]
fn doctor_fails_when_git_filters_cannot_run() {
    let tmp = healthy_repo();
    let cb_dir = LocalConfig::commitbook_dir(tmp.path());
    std::fs::write(tmp.path().join(".gitattributes"), "*.psd filter=lfs\n").unwrap();
    assert!(run(&cb_dir, tmp.path(), false, false).is_err());
}

#[test]
fn doctor_json_reports_failed_checks_without_text_formatting() {
    let tmp = healthy_repo();
    let cb_dir = LocalConfig::commitbook_dir(tmp.path());
    std::fs::write(LocalConfig::config_path(tmp.path()), "invalid = [").unwrap();

    let report = diagnose(&cb_dir, tmp.path());
    let json: serde_json::Value = serde_json::from_str(&report.json().unwrap()).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["repairs"], serde_json::json!([]));
    let config_check = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "config.toml")
        .unwrap();
    assert_eq!(config_check["status"], "invalid");
    assert_eq!(config_check["failed"], true);
    assert!(config_check["details"][0]
        .as_str()
        .unwrap()
        .contains("Invalid config"));
}

#[test]
fn doctor_fix_refuses_contended_lock_without_running_repairs() {
    let tmp = healthy_repo();
    let cb_dir = LocalConfig::commitbook_dir(tmp.path());
    let logs = LocalConfig::logs_dir(tmp.path());
    std::fs::remove_dir_all(&logs).unwrap();
    let held = RepoLock::acquire(tmp.path()).unwrap();

    let report = build_report(&cb_dir, tmp.path(), true);
    assert!(!report.ok());
    assert_eq!(report.repairs.len(), 1);
    assert_eq!(report.repairs[0].name, "Repository lock");
    assert!(report.repairs[0].failed);

    drop(held);
    run(&cb_dir, tmp.path(), true, true).unwrap();
    assert!(logs.is_dir());
}

#[test]
fn doctor_fix_reports_logs_directory_recreated_during_lock_setup() {
    let tmp = healthy_repo();
    let cb_dir = LocalConfig::commitbook_dir(tmp.path());
    let logs = LocalConfig::logs_dir(tmp.path());
    std::fs::remove_dir_all(&logs).unwrap();

    let report = build_report(&cb_dir, tmp.path(), true);
    assert!(report.ok());
    assert!(logs.is_dir());
    assert!(report
        .repairs
        .iter()
        .any(|repair| repair.name == "Creating logs/" && repair.status == "applied"));
}
