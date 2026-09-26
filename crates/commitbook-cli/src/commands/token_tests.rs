use super::*;

#[test]
fn test_detect_provider_no_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(detect_provider(tmp.path()), "generic");
}

#[test]
fn test_detect_provider_no_remote() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "generic");
}

#[test]
fn test_detect_provider_github() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/user/repo.git",
        ])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "github");
}

#[test]
fn test_detect_provider_gitlab() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://gitlab.com/user/repo.git",
        ])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "gitlab");
}

#[test]
fn test_detect_provider_codeberg() {
    let tmp = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://codeberg.org/user/repo.git",
        ])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(detect_provider(tmp.path()), "codeberg");
}

#[test]
fn parse_token_trims_one_line_and_rejects_blank_or_spaced_input() {
    assert_eq!(parse_token("  ghp_abc123\n").unwrap(), "ghp_abc123");
    assert!(parse_token("\n").is_err());
    assert!(parse_token("ghp_abc 123").is_err());
}

#[test]
fn token_set_takes_no_token_argument() {
    use clap::Parser;
    // Arguments end up in shell history and `ps`; only stdin is accepted.
    assert!(
        crate::Cli::try_parse_from(["commitbook", "token", "set", "--token", "ghp_x"]).is_err()
    );
    assert!(
        crate::Cli::try_parse_from(["commitbook", "token", "set", "--provider", "github"]).is_ok()
    );
}

#[test]
fn read_token_takes_the_first_line_of_piped_input() {
    let token = read_token(&mut "ghp_piped\nignored second line\n".as_bytes()).unwrap();
    assert_eq!(token, "ghp_piped");
}
