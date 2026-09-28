use super::*;
use std::path::Path;

fn entry(repo: &str, bin: &str) -> (String, String) {
    build_crontab_entry(Path::new(repo), "0 * * * *", Path::new(bin), &[]).unwrap()
}

#[test]
fn entry_quotes_paths_as_single_shell_words() {
    let (comment, line) = entry("/tmp/repo", "/usr/bin/commitbook");
    assert_eq!(comment, "# CommitBook: /tmp/repo");
    assert_eq!(
        line,
        "0 * * * * cd '/tmp/repo' && '/usr/bin/commitbook' sync"
    );

    let (_, line) = entry("/home/u/my notes", "/usr/local/bin/my commitbook");
    assert_eq!(
        line,
        "0 * * * * cd '/home/u/my notes' && '/usr/local/bin/my commitbook' sync"
    );
}

#[test]
fn entry_keeps_shell_metacharacters_literal() {
    // `$`, backticks and `"` stay literal inside single quotes; a `'` is
    // closed, escaped, and reopened.
    let (_, line) = entry("/home/u/it's $HOME `x` \"q\"", "/usr/bin/commitbook");
    assert_eq!(
        line,
        r#"0 * * * * cd '/home/u/it'\''s $HOME `x` "q"' && '/usr/bin/commitbook' sync"#
    );
    assert_eq!(
        parse_entry(&line),
        Some((
            "/home/u/it's $HOME `x` \"q\"".to_string(),
            "/usr/bin/commitbook".to_string()
        ))
    );
}

#[test]
fn entry_refuses_paths_cron_cannot_carry() {
    for repo in ["/tmp/100%notes", "/tmp/line\nbreak"] {
        assert!(
            build_crontab_entry(
                Path::new(repo),
                "0 * * * *",
                Path::new("/usr/bin/commitbook"),
                &[]
            )
            .is_err(),
            "{repo:?}"
        );
    }
}

#[test]
fn filter_removes_our_entry_and_its_marker() {
    let (comment, line) = entry("/tmp/repo", "/usr/bin/commitbook");
    let crontab = format!("{comment}\n{line}\n");
    assert!(filter_crontab_lines(&crontab, Path::new("/tmp/repo")).is_empty());
}

#[test]
fn filter_removes_entries_written_by_older_releases() {
    let crontab = concat!(
        "# CommitBook: /tmp/repo\n",
        "0 * * * * cd \"/tmp/repo\" && \"/usr/bin/commitbook\" run\n",
        "5 * * * * cd \"/tmp/repo\" && \"/usr/bin/commitbook\" sync\n",
    );
    assert!(filter_crontab_lines(crontab, Path::new("/tmp/repo")).is_empty());
}

#[test]
fn filter_keeps_every_user_line() {
    let (comment, line) = entry("/tmp/repo", "/usr/bin/commitbook");
    let crontab = format!(
        concat!(
            "MAILTO=me@example.com\n",
            "0 9 * * * /usr/bin/backup\n",
            "# My note about CommitBook\n",
            // A user's own job in the same repo that mentions commitbook.
            "30 2 * * * cd \"/tmp/repo\" && git gc # commitbook repo\n",
            // A hand-written marker above an unrelated job must not eat it.
            "# CommitBook: /tmp/repo\n",
            "15 3 * * * /usr/bin/other-task\n",
            "{}\n{}\n",
        ),
        comment, line
    );
    let kept = filter_crontab_lines(&crontab, Path::new("/tmp/repo"));
    assert_eq!(
        kept,
        concat!(
            "MAILTO=me@example.com\n",
            "0 9 * * * /usr/bin/backup\n",
            "# My note about CommitBook\n",
            "30 2 * * * cd \"/tmp/repo\" && git gc # commitbook repo\n",
            "# CommitBook: /tmp/repo\n",
            "15 3 * * * /usr/bin/other-task",
        )
    );
}

#[test]
fn filter_does_not_match_a_repository_with_a_longer_path() {
    let (comment, line) = entry("/tmp/repository", "/usr/bin/commitbook");
    let crontab = format!("{comment}\n{line}");
    assert_eq!(
        filter_crontab_lines(&crontab, Path::new("/tmp/repo")),
        crontab
    );
}

#[test]
fn filter_empty_input() {
    assert!(filter_crontab_lines("", Path::new("/tmp/repo")).is_empty());
}

#[test]
fn crontab_binary_reads_new_and_old_entries() {
    let repo = Path::new("/tmp/repo");
    let (comment, line) = entry("/tmp/repo", "/opt/cb/target/debug/commitbook");
    let crontab = format!("0 1 * * * other-job\n{comment}\n{line}\n");
    assert_eq!(
        crontab_binary(&crontab, repo),
        Some(PathBuf::from("/opt/cb/target/debug/commitbook"))
    );
    assert_eq!(crontab_binary(&crontab, Path::new("/tmp/other")), None);

    let old = "0 * * * * cd \"/tmp/repo\" && \"/usr/bin/commitbook\" run\n";
    assert_eq!(
        crontab_binary(old, repo),
        Some(PathBuf::from("/usr/bin/commitbook"))
    );
}

#[cfg(unix)]
#[test]
fn crontab_read_failure_is_an_error_not_an_empty_crontab() {
    use std::os::unix::process::ExitStatusExt;
    let output = |code: i32, stderr: &str| std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    };
    assert_eq!(
        read_crontab_output(&output(1, "no crontab for alice\n")).unwrap(),
        ""
    );
    assert!(read_crontab_output(&output(1, "crontab: permission denied\n")).is_err());
}

#[test]
fn entry_sets_path_and_ssh_agent_for_commitbook_only() {
    let environment = [
        ("PATH", "/home/u/.local/bin:/usr/bin:/bin".to_string()),
        (
            "SSH_AUTH_SOCK",
            "/run/user/1000/ssh-agent.socket".to_string(),
        ),
    ];
    let (_, line) = build_crontab_entry(
        Path::new("/tmp/repo"),
        "0 * * * *",
        Path::new("/usr/bin/commitbook"),
        &environment,
    )
    .unwrap();
    assert_eq!(
        line,
        "0 * * * * cd '/tmp/repo' && PATH='/home/u/.local/bin:/usr/bin:/bin' \
         SSH_AUTH_SOCK='/run/user/1000/ssh-agent.socket' '/usr/bin/commitbook' sync"
    );
    // The entry is still recognized, so reinstall and uninstall find it.
    assert_eq!(
        crontab_binary(&line, Path::new("/tmp/repo")),
        Some(PathBuf::from("/usr/bin/commitbook"))
    );
    assert!(filter_crontab_lines(&line, Path::new("/tmp/repo")).is_empty());
}

#[test]
fn entry_leaves_out_values_cron_cannot_carry() {
    let environment = [("SSH_AUTH_SOCK", "/tmp/100%agent".to_string())];
    let (_, line) = build_crontab_entry(
        Path::new("/tmp/repo"),
        "0 * * * *",
        Path::new("/usr/bin/commitbook"),
        &environment,
    )
    .unwrap();
    assert_eq!(
        line,
        "0 * * * * cd '/tmp/repo' && '/usr/bin/commitbook' sync"
    );
}

#[test]
fn scheduled_environment_has_a_usable_path() {
    let environment = scheduled_environment();
    let path = &environment
        .iter()
        .find(|(name, _)| *name == "PATH")
        .unwrap()
        .1;
    for dir in ["/usr/bin", "/bin"] {
        assert!(path.split(':').any(|entry| entry == dir), "{path}");
    }
    assert!(environment.iter().all(|(_, value)| is_cron_safe(value)));
}

#[test]
fn unrelated_and_commented_jobs_are_neither_removed_nor_reported() {
    let repo = Path::new("/tmp/notes");
    for binary in [
        "/usr/local/bin/commitbook-backup",
        "/opt/commitbook-helper",
        "commitbook",
        "./commitbook",
    ] {
        for command in ["run", "sync"] {
            // Even a marker cannot make another executable our job.
            let line = format!(
                "# CommitBook: /tmp/notes\n0 * * * * cd \"/tmp/notes\" && \"{binary}\" {command}"
            );
            assert_eq!(filter_crontab_lines(&line, repo), line);
            assert_eq!(crontab_binary(&line, repo), None);
        }
    }
    let (_, active) = entry("/tmp/notes", "/usr/bin/commitbook");
    for prefix in ["#", "  # "] {
        let commented = format!("{prefix}{active}");
        assert_eq!(filter_crontab_lines(&commented, repo), commented);
        assert_eq!(crontab_binary(&commented, repo), None);
        let mixed = format!("{commented}\n{active}");
        assert_eq!(filter_crontab_lines(&mixed, repo), commented);
        assert_eq!(
            crontab_binary(&mixed, repo),
            Some(PathBuf::from("/usr/bin/commitbook"))
        );
    }
}
