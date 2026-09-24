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
