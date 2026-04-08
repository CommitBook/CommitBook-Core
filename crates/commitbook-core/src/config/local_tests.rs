use super::*;

#[test]
fn test_default_field_values() {
    let cfg = LocalConfig::new("0 * * * *");
    assert!(cfg.enabled);
    assert_eq!(cfg.schedule, "0 * * * *");
    assert!(cfg.last_commit.is_none());
    assert!(cfg.scheduler_id.is_none());
    assert!(cfg.git.auto_push);
    assert_eq!(cfg.git.branch, "main");
    assert_eq!(cfg.logging.level, "info");
    assert_eq!(cfg.logging.max_log_files, 30);
}

#[test]
fn test_commitbook_dir_path() {
    let dir = LocalConfig::commitbook_dir(Path::new("/tmp/repo"));
    assert_eq!(dir, PathBuf::from("/tmp/repo/.CommitBook"));
}

#[test]
fn test_config_path() {
    let p = LocalConfig::config_path(Path::new("/tmp/repo"));
    assert_eq!(p, PathBuf::from("/tmp/repo/.CommitBook/config.toml"));
}

#[test]
fn test_logs_dir_path() {
    let p = LocalConfig::logs_dir(Path::new("/tmp/repo"));
    assert_eq!(p, PathBuf::from("/tmp/repo/.CommitBook/logs"));
}

#[test]
fn test_lock_path() {
    let p = LocalConfig::lock_path(Path::new("/tmp/repo"));
    assert_eq!(p, PathBuf::from("/tmp/repo/.CommitBook/.lock"));
}

#[test]
fn test_save_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    let original = LocalConfig::new("*/15 * * * *");
    original.save(repo).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert_eq!(loaded.schedule, "*/15 * * * *");
    assert!(loaded.enabled);
    assert_eq!(loaded.git.branch, "main");
}

#[test]
fn test_load_nonexistent_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let result = LocalConfig::load(tmp.path());
    assert!(result.is_err());
}

#[test]
fn test_init_creates_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    let cfg = LocalConfig::init(repo, "hourly").unwrap();
    assert_eq!(cfg.schedule, "hourly");

    // Directories created
    assert!(LocalConfig::commitbook_dir(repo).exists());
    assert!(LocalConfig::logs_dir(repo).exists());

    // Config file created
    assert!(LocalConfig::config_path(repo).exists());

    // .gitignore updated
    let gitignore = fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gitignore.contains(".CommitBook/logs/"));
    assert!(gitignore.contains(".CommitBook/.lock"));
}

#[test]
fn test_new_has_config_version() {
    let cfg = LocalConfig::new("hourly");
    assert_eq!(cfg.config_version, "1");
}

#[test]
fn test_load_without_version_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    // Write TOML without config_version field
    let toml_content = r#"
enabled = true
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert_eq!(loaded.config_version, "1");
}

#[test]
fn test_migrate_returns_false_when_current() {
    let mut cfg = LocalConfig::new("hourly");
    assert!(!cfg.migrate());
}
