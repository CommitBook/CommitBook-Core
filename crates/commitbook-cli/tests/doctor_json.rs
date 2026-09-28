use std::path::Path;
use std::process::Command;

use commitbook_engine::config::LocalConfig;

fn git(root: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap()
        .success());
}

#[test]
fn doctor_json_is_a_single_parseable_report_on_success_and_failure() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    git(
        root.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/notes.git",
        ],
    );
    LocalConfig::init(root.path(), &LocalConfig::new("notes", "main", "origin")).unwrap();

    let doctor = || {
        Command::new(env!("CARGO_BIN_EXE_commitbook"))
            .args(["--json", "doctor"])
            .current_dir(root.path())
            .output()
            .unwrap()
    };
    let healthy = doctor();
    assert!(healthy.status.success());
    let healthy_json: serde_json::Value = serde_json::from_slice(&healthy.stdout).unwrap();
    assert_eq!(healthy_json["ok"], true);
    assert!(healthy_json["checks"].is_array());

    std::fs::write(LocalConfig::config_path(root.path()), "invalid = [").unwrap();
    let broken = doctor();
    assert!(!broken.status.success());
    let broken_json: serde_json::Value = serde_json::from_slice(&broken.stdout).unwrap();
    assert_eq!(broken_json["ok"], false);
    assert!(broken_json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "config.toml" && check["failed"] == true));
}
