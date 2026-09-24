use super::*;

fn config() -> LocalConfig {
    LocalConfig::new("notes", "main", "origin")
}

fn init(repo: &Path) -> LocalConfig {
    let config = config();
    LocalConfig::init(repo, &config).unwrap();
    config
}

#[test]
fn new_config_has_the_documented_defaults() {
    let cfg = config();
    assert_eq!(cfg.config.schema, CONFIG_SCHEMA);
    assert_eq!(cfg.commitbook.name, "notes");
    assert_eq!(cfg.git.branch, "main");
    assert_eq!(cfg.git.remote, "origin");
    assert_eq!(cfg.sync.schedule, "1h");
    assert_eq!(cfg.commit.mode, CommitMode::Timestamp);
    assert_eq!(cfg.commit.agent, CommitAgent::Any);
    assert_eq!(cfg.conflicts.mode, ConflictMode::Both);
    assert_eq!(cfg.conflicts.agent, Agent::Claude);
    assert_eq!(cfg.logs.keep, LogKeep::Days(30));
}

#[test]
fn path_helpers_point_into_commitbook_dir() {
    let repo = Path::new("/tmp/repo");
    assert_eq!(
        LocalConfig::commitbook_dir(repo),
        PathBuf::from("/tmp/repo/.CommitBook")
    );
    assert_eq!(
        LocalConfig::config_path(repo),
        PathBuf::from("/tmp/repo/.CommitBook/config.toml")
    );
    assert_eq!(
        LocalConfig::logs_dir(repo),
        PathBuf::from("/tmp/repo/.CommitBook/local/logs")
    );
    assert_eq!(
        LocalConfig::lock_path(repo),
        PathBuf::from("/tmp/repo/.CommitBook/local/.lock")
    );
}

#[test]
fn init_writes_the_commented_template_in_section_order() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());

    let content = fs::read_to_string(LocalConfig::config_path(tmp.path())).unwrap();
    assert!(content.starts_with("# CommitBook settings."), "{content}");
    let sections = [
        "[config]",
        "[commitbook]",
        "[git]",
        "[sync]",
        "[commit]",
        "[conflicts]",
        "[logs]",
    ];
    let positions: Vec<usize> = sections
        .iter()
        .map(|section| content.find(section).unwrap_or_else(|| panic!("{section}")))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{content}"
    );
    assert!(content.contains("name = \"notes\""), "{content}");
    assert!(content.contains("# both: keep both versions"), "{content}");
    assert!(
        content.find("branch = ").unwrap() < content.find("remote = ").unwrap(),
        "branch comes before remote"
    );
    assert_eq!(LocalConfig::load(tmp.path()).unwrap(), config());
}

#[test]
fn init_creates_structure_and_leaves_the_root_gitignore_alone() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join(".gitignore"), "user-owned/\n").unwrap();
    init(tmp.path());

    assert!(LocalConfig::logs_dir(tmp.path()).exists());
    assert_eq!(
        fs::read_to_string(tmp.path().join(".CommitBook/.gitignore")).unwrap(),
        "/local/\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join(".gitignore")).unwrap(),
        "user-owned/\n"
    );
}

#[test]
fn save_edits_values_in_place_and_keeps_user_comments() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = init(tmp.path());
    let path = LocalConfig::config_path(tmp.path());
    let content = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        content.replace("[sync]\n", "[sync]\n# my note about timing\n"),
    )
    .unwrap();

    config.sync.schedule = "15m".into();
    config.conflicts.mode = ConflictMode::Review;
    config.save(tmp.path()).unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("# my note about timing"), "{saved}");
    assert!(saved.contains("schedule = \"15m\""), "{saved}");
    let mode_line = saved
        .lines()
        .find(|line| line.starts_with("mode = \"review\""))
        .unwrap();
    assert!(
        mode_line.contains("# both: keep both versions"),
        "the value's trailing comment survives:\n{saved}"
    );
    assert_eq!(LocalConfig::load(tmp.path()).unwrap(), config);
}

#[test]
fn trailing_comments_keep_their_column_when_values_change_width() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = LocalConfig::new("Personal Notes", "main", "origin");
    LocalConfig::init(tmp.path(), &config).unwrap();
    config.conflicts.mode = ConflictMode::Ai;
    config.save(tmp.path()).unwrap();

    let saved = fs::read_to_string(LocalConfig::config_path(tmp.path())).unwrap();
    let column = |prefix: &str| {
        let line = saved.lines().find(|l| l.starts_with(prefix)).unwrap();
        line.find('#').unwrap()
    };
    assert_eq!(column("name = "), column("schema = "), "{saved}");
    assert_eq!(column("mode = \"ai\""), column("schema = "), "{saved}");
}

#[test]
fn logs_section_is_optional() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());
    let path = LocalConfig::config_path(tmp.path());
    let content = fs::read_to_string(&path).unwrap();
    let without_logs = &content[..content.find("[logs]").unwrap()];
    fs::write(&path, without_logs).unwrap();

    assert_eq!(
        LocalConfig::load(tmp.path()).unwrap().logs.keep,
        LogKeep::default()
    );
}

#[test]
fn invalid_values_name_the_allowed_ones() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());
    let path = LocalConfig::config_path(tmp.path());
    let content = fs::read_to_string(&path).unwrap();
    fs::write(&path, content.replace("mode = \"both\"", "mode = \"auto\"")).unwrap();

    let error = format!("{:#}", LocalConfig::load(tmp.path()).unwrap_err());
    assert!(error.contains("auto"), "{error}");
    assert!(error.contains("both"), "{error}");
}

#[test]
fn unknown_keys_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());
    let path = LocalConfig::config_path(tmp.path());
    let content = fs::read_to_string(&path).unwrap();
    fs::write(&path, content.replace("[sync]\n", "[sync]\npush = false\n")).unwrap();

    let error = format!("{:#}", LocalConfig::load(tmp.path()).unwrap_err());
    assert!(error.contains("push"), "{error}");
}

#[test]
fn log_keep_rejects_zero_and_bare_numbers() {
    for keep in ["0d", "7"] {
        let text = TEMPLATE
            .replace("name = \"\"", "name = \"notes\"")
            .replace("keep = \"30d\"", &format!("keep = \"{keep}\""));
        assert!(LocalConfig::parse(&text).is_err(), "{keep}");
    }
    let forever = TEMPLATE
        .replace("name = \"\"", "name = \"notes\"")
        .replace("keep = \"30d\"", "keep = \"forever\"");
    assert_eq!(
        LocalConfig::parse(&forever).unwrap().logs.keep,
        LogKeep::Forever
    );
}

#[test]
fn files_in_another_format_ask_for_reinit() {
    for text in [
        "config_version = \"1\"\nenabled = true\nschedule = \"0 * * * *\"\n",
        "[config]\nschema = 2\n",
        "",
    ] {
        let error = LocalConfig::parse(text).unwrap_err().to_string();
        assert!(error.contains("commitbook init"), "{text:?}: {error}");
    }
}

#[test]
fn empty_name_is_rejected() {
    let error = LocalConfig::parse(TEMPLATE).unwrap_err().to_string();
    assert!(error.contains("name"), "{error}");
}

#[test]
fn load_nonexistent_errors() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(LocalConfig::load(tmp.path()).is_err());
}

#[cfg(unix)]
#[test]
fn load_rejects_symlinked_commitbook_directory() {
    use std::os::unix::fs::symlink;

    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    init(outside.path());
    symlink(
        outside.path().join(".CommitBook"),
        repo.path().join(".CommitBook"),
    )
    .unwrap();

    assert!(LocalConfig::load(repo.path()).is_err());
    assert!(!LocalConfig::exists(repo.path()));
}

#[cfg(unix)]
#[test]
fn config_io_rejects_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".CommitBook")).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "outside sentinel\n").unwrap();
    symlink(outside.path(), repo.path().join(".CommitBook/config.toml")).unwrap();

    assert!(LocalConfig::load(repo.path()).is_err());
    assert!(config().save(repo.path()).is_err());
    assert_eq!(
        fs::read_to_string(outside.path()).unwrap(),
        "outside sentinel\n"
    );
}

#[cfg(unix)]
#[test]
fn gitignore_io_rejects_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".CommitBook")).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "outside sentinel\n").unwrap();
    symlink(outside.path(), repo.path().join(".CommitBook/.gitignore")).unwrap();

    assert!(LocalConfig::ensure_gitignore(repo.path()).is_err());
    assert_eq!(
        fs::read_to_string(outside.path()).unwrap(),
        "outside sentinel\n"
    );
}

#[cfg(unix)]
#[test]
fn save_replaces_atomically_and_preserves_mode() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = init(tmp.path());
    let path = LocalConfig::config_path(tmp.path());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    config.sync.schedule = "5m".to_string();
    config.save(tmp.path()).unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "existing permissions must be preserved");
    assert_eq!(LocalConfig::load(tmp.path()).unwrap().sync.schedule, "5m");
    let leftovers: Vec<_> = fs::read_dir(LocalConfig::commitbook_dir(tmp.path()))
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no temporary files may remain");
}

#[cfg(unix)]
#[test]
fn atomic_write_failure_leaves_previous_file_intact() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "original = true\n").unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();

    let result = write_regular_text_atomic(&path, "replacement = true\n");

    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    if nix_is_root() {
        // Root ignores directory permission bits; the write succeeds there.
        return;
    }
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "original = true\n");
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "failed writes must not leave temp files"
    );
}

#[cfg(unix)]
fn nix_is_root() -> bool {
    // SAFETY: geteuid has no preconditions and only reads process state.
    unsafe { libc::geteuid() == 0 }
}

#[test]
fn atomic_write_refuses_directory_destination() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::create_dir(&path).unwrap();

    let error = write_regular_text_atomic(&path, "x = 1\n").unwrap_err();
    assert!(error.to_string().contains("non-regular"), "{error}");
    assert!(path.is_dir(), "destination directory must be untouched");
}

#[test]
fn save_sweeps_stale_temp_files() {
    let tmp = tempfile::tempdir().unwrap();
    let config = init(tmp.path());
    let cb_dir = LocalConfig::commitbook_dir(tmp.path());
    let stale = cb_dir.join(".config.toml.abc123.tmp");
    fs::write(&stale, "partial").unwrap();
    let unrelated = cb_dir.join("notes.tmp");
    fs::write(&unrelated, "keep").unwrap();

    config.save(tmp.path()).unwrap();

    assert!(!stale.exists(), "stale config temp file must be swept");
    assert!(unrelated.exists(), "unrelated files must be left alone");
}
