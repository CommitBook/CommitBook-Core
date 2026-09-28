use super::*;

fn create_temp_repo() -> (tempfile::TempDir, git2::Repository) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    (tmp, repo)
}

#[test]
fn test_get_remote_url_no_remote_errors() {
    let (tmp, _repo) = create_temp_repo();
    let result = get_remote_url(tmp.path(), "origin");
    assert!(result.is_err());
}

#[test]
fn test_get_remote_url_with_remote() {
    let (tmp, repo) = create_temp_repo();
    repo.remote("origin", "https://example.com/repo.git")
        .unwrap();

    let url = get_remote_url(tmp.path(), "origin").unwrap();
    assert_eq!(url, "https://example.com/repo.git");
}

#[test]
fn clone_urls_hide_credentials_and_match_across_transports() {
    assert!(same_remote(
        "https://github.com/Owner/Notes.git",
        "git@github.com:owner/notes.git"
    ));
    assert!(!same_remote(
        "https://gitlab.com/owner/notes.git",
        "https://github.com/owner/notes.git"
    ));
    assert_eq!(
        credential_free_url("https://user:secret@example.com/team/notes.git?token=hidden").unwrap(),
        "https://example.com/team/notes.git"
    );
    assert!(validate_clone_url("https://user:secret@example.com/notes.git").is_err());
    assert!(validate_clone_url("https://example.com/notes.git?token=secret").is_err());
    assert!(validate_clone_url("git@example.com:team/notes.git").is_ok());
}

#[test]
fn different_local_remotes_with_the_same_filename_are_distinct() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let a = first.path().join("notes.git");
    let b = second.path().join("notes.git");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    assert!(same_remote(
        a.to_str().unwrap(),
        &format!("file://{}", a.display())
    ));
    assert!(!same_remote(a.to_str().unwrap(), b.to_str().unwrap()));
}

#[test]
fn parses_github_https_and_ssh_urls() {
    for url in [
        "https://github.com/manuel/notes.git",
        "https://github.com/manuel/notes",
        "git@github.com:manuel/notes.git",
        "ssh://git@github.com/manuel/notes.git",
    ] {
        let identity = parse_remote_url(url).unwrap();
        assert_eq!(identity.provider, Provider::Github, "{url}");
        assert_eq!(identity.owner, "manuel", "{url}");
        assert_eq!(identity.repo, "notes", "{url}");
    }
    assert!(
        !parse_remote_url("https://github.com/manuel/notes")
            .unwrap()
            .ssh
    );
    assert!(
        parse_remote_url("git@github.com:manuel/notes.git")
            .unwrap()
            .ssh
    );
}

#[test]
fn ignores_credentials_embedded_in_https_urls() {
    let identity =
        parse_remote_url("https://x-access-token:SECRET@github.com/manuel/notes.git").unwrap();
    assert_eq!(identity.provider, Provider::Github);
    assert_eq!(identity.owner, "manuel");
    assert!(!format!("{identity:?}").contains("SECRET"));
}

#[test]
fn keeps_gitlab_subgroups_in_the_owner() {
    let identity = parse_remote_url("https://gitlab.com/group/sub/notes.git").unwrap();
    assert_eq!(identity.provider, Provider::Gitlab);
    assert_eq!(identity.owner, "group/sub");
    assert_eq!(identity.repo, "notes");
}

#[test]
fn recognizes_codeberg_and_self_hosted_hosts() {
    let codeberg = parse_remote_url("https://codeberg.org/manuel/notes").unwrap();
    assert_eq!(codeberg.provider, Provider::Codeberg);
    let hosted = parse_remote_url("git@git.example.com:team/notes.git").unwrap();
    assert_eq!(hosted.provider, Provider::GenericGit);
    assert_eq!(hosted.owner, "team");
    assert_eq!(hosted.repo, "notes");
}

#[test]
fn local_paths_use_the_parent_folder_as_owner() {
    for url in [
        "/srv/git/notes.git",
        "file:///srv/git/notes.git",
        "../git/notes",
    ] {
        let identity = parse_remote_url(url).unwrap();
        assert_eq!(identity.provider, Provider::GenericGit, "{url}");
        assert_eq!(identity.owner, "git", "{url}");
        assert_eq!(identity.repo, "notes", "{url}");
    }
    assert_eq!(parse_remote_url("./notes.git").unwrap().owner, "");
}

#[test]
fn rejects_urls_without_a_repository_name() {
    assert!(parse_remote_url("").is_err());
    assert!(parse_remote_url("https://github.com/").is_err());
}

#[test]
fn remote_identity_reads_the_named_remote() {
    let (tmp, repo) = create_temp_repo();
    repo.remote("upstream", "git@github.com:manuel/notes.git")
        .unwrap();
    let identity = remote_identity(tmp.path(), "upstream").unwrap();
    assert_eq!(identity.repo, "notes");
    assert!(remote_identity(tmp.path(), "origin").is_err());
}

#[test]
fn windows_drive_paths_and_file_urls_are_local_on_every_platform() {
    for (url, owner) in [
        ("C:/notes.git", ""),
        (r"C:\notes.git", ""),
        ("c:/books/notes.git", "books"),
        (r"D:\books\notes.git", "books"),
        ("file:///C:/notes.git", ""),
        ("file:///C:/books/notes.git", "books"),
        ("file:/C:/books/notes.git", "books"),
        ("file:C:/books/notes.git", "books"),
        (r"file:///C:\books\notes.git", "books"),
        ("FILE:///c:/books/notes.git", "books"),
    ] {
        let identity = parse_remote_url(url).unwrap();
        assert_eq!(identity.provider, Provider::GenericGit, "{url}");
        assert_eq!(identity.owner, owner, "{url}");
        assert_eq!(identity.repo, "notes", "{url}");
        assert!(!identity.ssh, "{url}");
    }
}

#[test]
fn local_path_normalization_preserves_scp_and_posix_paths() {
    for url in [
        "host:books/notes.git",
        "git@host:books/notes.git",
        "ssh://host/books/notes.git",
    ] {
        let identity = parse_remote_url(url).unwrap();
        assert!(identity.ssh, "{url}");
        assert_eq!(identity.owner, "books");
        assert_eq!(identity.repo, "notes");
    }
    for url in [
        r"/srv/books\archive/notes.git",
        r"file:///srv/books\archive/notes.git",
    ] {
        let identity = parse_remote_url(url).unwrap();
        assert!(!identity.ssh);
        assert_eq!(identity.owner, r"books\archive");
        assert_eq!(identity.repo, "notes");
    }
}

#[test]
fn retains_actual_hostname_without_credentials() {
    let identity =
        parse_remote_url("https://user:secret@Git.Example.org:8443/team/notes.git").unwrap();
    assert_eq!(identity.host.as_deref(), Some("git.example.org"));
    assert!(!format!("{identity:?}").contains("secret"));
    assert_eq!(parse_remote_url("/srv/git/notes.git").unwrap().host, None);
}
