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
