use super::*;
use std::path::Path;

#[test]
fn test_build_entry_format() {
    let (comment, entry) = build_crontab_entry(
        Path::new("/tmp/repo"),
        "0 * * * *",
        Path::new("/usr/bin/commitbook"),
    );
    assert_eq!(comment, "# CommitBook: /tmp/repo");
    assert_eq!(
        entry,
        "0 * * * * cd \"/tmp/repo\" && \"/usr/bin/commitbook\" run"
    );
}

#[test]
fn test_build_entry_different_schedule() {
    let (_, entry) = build_crontab_entry(
        Path::new("/home/user/notes"),
        "*/15 * * * *",
        Path::new("/usr/local/bin/commitbook"),
    );
    assert!(entry.starts_with("*/15 * * * *"));
    assert!(entry.contains("/usr/local/bin/commitbook"));
    assert!(entry.contains("/home/user/notes"));
}

#[test]
fn test_build_entry_paths_with_spaces() {
    let (comment, entry) = build_crontab_entry(
        Path::new("/home/user/my notes"),
        "0 * * * *",
        Path::new("/usr/local/bin/my commitbook"),
    );
    assert_eq!(comment, "# CommitBook: /home/user/my notes");
    assert_eq!(
        entry,
        "0 * * * * cd \"/home/user/my notes\" && \"/usr/local/bin/my commitbook\" run"
    );
}

#[test]
fn test_filter_removes_entry() {
    let crontab =
        "# CommitBook: /tmp/repo\n0 * * * * cd \"/tmp/repo\" && \"/usr/bin/commitbook\" run\n";
    let result = filter_crontab_lines(crontab, Path::new("/tmp/repo"));
    assert!(result.trim().is_empty());
}

#[test]
fn test_filter_preserves_unrelated() {
    let crontab = "0 9 * * * /usr/bin/backup\n# CommitBook: /tmp/repo\n0 * * * * cd \"/tmp/repo\" && \"/usr/bin/commitbook\" run\n30 * * * * /usr/bin/other-task";
    let result = filter_crontab_lines(crontab, Path::new("/tmp/repo"));
    assert!(result.contains("/usr/bin/backup"));
    assert!(result.contains("/usr/bin/other-task"));
    assert!(!result.contains("CommitBook"));
}

#[test]
fn test_filter_empty_input() {
    let result = filter_crontab_lines("", Path::new("/tmp/repo"));
    assert!(result.is_empty());
}

#[test]
fn test_filter_no_match() {
    let crontab = "0 9 * * * /usr/bin/backup\n30 * * * * /usr/bin/other";
    let result = filter_crontab_lines(crontab, Path::new("/tmp/repo"));
    assert_eq!(result, crontab);
}

#[test]
fn test_filter_no_prefix_false_positive() {
    // Filtering /tmp/repo must NOT remove an entry for /tmp/repository
    let crontab = concat!(
        "# CommitBook: /tmp/repository\n",
        "0 * * * * cd \"/tmp/repository\" && \"/usr/bin/commitbook\" run\n",
    );
    let result = filter_crontab_lines(crontab, Path::new("/tmp/repo"));
    assert!(
        result.contains("/tmp/repository"),
        "Entry for /tmp/repository should be preserved when filtering /tmp/repo"
    );
}
