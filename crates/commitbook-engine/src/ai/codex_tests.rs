use super::*;
use crate::ai::CommitMessageProvider;
use crate::ai::ConflictResolver;

#[test]
fn test_codex_commit_provider_name() {
    let provider = CodexProvider;
    assert_eq!(CommitMessageProvider::name(&provider), "Codex");
}

#[test]
fn test_codex_commit_provider_key() {
    let provider = CodexProvider;
    assert_eq!(CommitMessageProvider::key(&provider), "codex-cli");
}

#[test]
fn test_codex_resolver_key() {
    let provider = CodexProvider;
    assert_eq!(ConflictResolver::key(&provider), "codex");
}

#[test]
fn test_codex_is_available_returns_bool() {
    let provider = CodexProvider;
    let _commit: bool = CommitMessageProvider::is_available(&provider);
    let _resolve: bool = ConflictResolver::is_available(&provider);
}

#[test]
fn test_codex_command_uses_noninteractive_read_only_contract() {
    let command = command(Path::new("/tmp/repo"), Path::new("/tmp/last-message"));
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert_eq!(
        args,
        [
            "exec",
            "--ephemeral",
            "--sandbox",
            "read-only",
            "--color",
            "never",
            "--output-last-message",
            "/tmp/last-message",
            "-",
        ]
    );
    assert!(!args.iter().any(|arg| arg == "--quiet"));
}
