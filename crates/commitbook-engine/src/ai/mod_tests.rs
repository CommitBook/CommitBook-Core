use super::*;

#[test]
fn test_clean_message_passthrough() {
    assert_eq!(clean_message("Fix login bug"), "Fix login bug");
}

#[test]
fn test_clean_message_strips_fences() {
    assert_eq!(clean_message("```Update README```"), "Update README");
}

#[test]
fn test_clean_message_strips_quotes() {
    assert_eq!(clean_message("\"Add tests\""), "Add tests");
    assert_eq!(clean_message("'Add tests'"), "Add tests");
    assert_eq!(clean_message("`Add tests`"), "Add tests");
}

#[test]
fn test_clean_message_first_line_only() {
    assert_eq!(
        clean_message("First line\nSecond line\nThird line"),
        "First line"
    );
}

#[test]
fn test_clean_message_truncates_to_72() {
    let long = "A".repeat(100);
    let result = clean_message(&long);
    assert_eq!(result.len(), 72);
}

#[test]
fn test_truncate_short_passthrough() {
    assert_eq!(truncate("hello", 10), "hello");
}

#[test]
fn test_truncate_long_appends_ellipsis() {
    assert_eq!(truncate("hello world", 5), "hello...");
}

#[test]
fn test_truncate_multibyte_does_not_panic() {
    // "🎉" is 4 bytes; truncating at byte 2 would panic without char-boundary check
    let s = "🎉🎉🎉";
    let result = truncate(s, 5);
    assert!(result.ends_with("..."));
    assert!(!result.contains('\u{FFFD}')); // no replacement chars
}

#[test]
fn test_clean_message_multibyte_truncate() {
    // 72 chars of emoji (each emoji = 4 bytes = 288 bytes total)
    let long_emoji = "😀".repeat(80);
    let result = clean_message(&long_emoji);
    // Should not panic, and result should be valid UTF-8
    assert!(result.len() <= 288); // 72 * 4 bytes max
    assert!(result.is_char_boundary(result.len()));
}

// --- looks_like_diff_narration tests ---

#[test]
fn test_diff_narration_detects_staged_unstaged_pair() {
    assert!(looks_like_diff_narration(
        "The staged change adds an entry; the unstaged adjusts a line"
    ));
}

#[test]
fn test_diff_narration_detects_prefix_the_diff() {
    assert!(looks_like_diff_narration(
        "The diff shows a few formatting tweaks"
    ));
}

#[test]
fn test_diff_narration_detects_prefix_this_diff() {
    assert!(looks_like_diff_narration(
        "This diff modifies multiple files"
    ));
}

#[test]
fn test_diff_narration_detects_prefix_the_changes_show() {
    assert!(looks_like_diff_narration(
        "The changes show new content added"
    ));
}

#[test]
fn test_diff_narration_case_insensitive() {
    assert!(looks_like_diff_narration(
        "THE STAGED CHANGES ADD X; THE UNSTAGED Y"
    ));
}

#[test]
fn test_diff_narration_passes_normal_messages() {
    assert!(!looks_like_diff_narration(
        "Add pagination to user list endpoint"
    ));
    assert!(!looks_like_diff_narration(
        "Fix race condition in sync pipeline"
    ));
    assert!(!looks_like_diff_narration(
        "Update README with install steps"
    ));
}

#[test]
fn test_diff_narration_lone_word_staged_is_ok() {
    // A legitimate message about staging behavior should not be rejected
    // unless BOTH "staged" and "unstaged" appear together.
    assert!(!looks_like_diff_narration(
        "Stage all changes before commit"
    ));
}

// --- MockProvider + ProviderChain async tests ---

struct MockProvider {
    name: &'static str,
    key: &'static str,
    available: bool,
    response: Option<&'static str>,
}

#[async_trait]
impl CommitMessageProvider for MockProvider {
    fn name(&self) -> &str {
        self.name
    }
    fn key(&self) -> &str {
        self.key
    }
    fn is_available(&self) -> bool {
        self.available
    }

    async fn generate(&self, _summary: &ChangesSummary, _repo_path: &Path) -> Result<String> {
        match self.response {
            Some(msg) => Ok(msg.to_string()),
            None => anyhow::bail!("mock error"),
        }
    }
}

#[tokio::test]
async fn test_chain_skips_unavailable() {
    let chain = ProviderChain {
        providers: vec![
            Box::new(MockProvider {
                name: "Unavail",
                key: "unavail",
                available: false,
                response: Some("nope"),
            }),
            Box::new(MockProvider {
                name: "Avail",
                key: "avail",
                available: true,
                response: Some("good msg"),
            }),
        ],
    };
    let summary = ChangesSummary {
        new_files: vec!["a.txt".into()],
        ..Default::default()
    };
    let (msg, provider) = chain
        .generate(
            &summary,
            &["unavail".into(), "avail".into()],
            Path::new("/tmp"),
        )
        .await;
    assert_eq!(msg, "good msg");
    assert_eq!(provider, "Avail");
}

#[tokio::test]
async fn test_chain_all_fail_uses_fallback() {
    let chain = ProviderChain {
        providers: vec![
            Box::new(MockProvider {
                name: "Bad",
                key: "bad",
                available: true,
                response: None,
            }),
            Box::new(fallback::FallbackProvider),
        ],
    };
    let summary = ChangesSummary {
        new_files: vec!["a.txt".into()],
        ..Default::default()
    };
    let (msg, provider) = chain
        .generate(&summary, &["bad".into()], Path::new("/tmp"))
        .await;
    assert_eq!(provider, "Fallback");
    assert!(!msg.is_empty());
}

#[test]
fn test_check_availability() {
    let chain = ProviderChain {
        providers: vec![
            Box::new(MockProvider {
                name: "Yes",
                key: "yes",
                available: true,
                response: None,
            }),
            Box::new(MockProvider {
                name: "No",
                key: "no",
                available: false,
                response: None,
            }),
        ],
    };
    let result = chain.check_availability(&["yes".into(), "no".into(), "missing".into()]);
    assert_eq!(result.len(), 3);
    assert!(result[0].2); // "yes" is available
    assert!(!result[1].2); // "no" is not
    assert!(!result[2].2); // "missing" is not
}

#[cfg(unix)]
mod run_with_prompt_tests {
    use super::super::run_with_prompt;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    #[test]
    fn run_with_prompt_echoes_multi_megabyte_stdin() {
        // Well past the pipe buffer: the writer must run concurrently with
        // the stdout reader or the child blocks on a full stdout pipe.
        let prompt = "0123456789ABCDEF\n".repeat(256 * 1024);
        let output = run_with_prompt(&mut sh("cat"), &prompt, Duration::from_secs(60)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, prompt.as_bytes());
    }

    #[test]
    fn run_with_prompt_drains_large_stdout_and_stderr_concurrently() {
        let script =
            "yes 0123456789ABCDEF | head -c 3000000; yes FEDCBA9876543210 | head -c 3000000 >&2";
        let output = run_with_prompt(&mut sh(script), "ignored", Duration::from_secs(60)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 3_000_000);
        assert_eq!(output.stderr.len(), 3_000_000);
    }

    #[test]
    fn run_with_prompt_tolerates_child_that_exits_without_reading_stdin() {
        let prompt = "x".repeat(1024 * 1024);
        let output = run_with_prompt(
            &mut sh("echo done; exit 0"),
            &prompt,
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"done\n");
    }

    #[test]
    fn run_with_prompt_kills_process_group_on_timeout() {
        let started = Instant::now();
        let error = run_with_prompt(
            &mut sh("sleep 60; echo late"),
            "ignored",
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error:#}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn run_with_prompt_reports_non_zero_exit_with_stderr() {
        let output = run_with_prompt(
            &mut sh("echo boom >&2; exit 3"),
            "ignored",
            Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stderr, b"boom\n");
    }
}

#[test]
fn ai_with_any_agent_tries_every_agent_then_fallback() {
    assert_eq!(
        commit_provider_keys(CommitMode::Ai, CommitAgent::Any),
        [
            "gh-copilot",
            "claude-cli",
            "codex-cli",
            "gemini-cli",
            "cursor-agent",
            "fallback"
        ]
    );
}

#[test]
fn ai_with_one_agent_tries_only_that_agent_then_fallback() {
    assert_eq!(
        commit_provider_keys(CommitMode::Ai, CommitAgent::Claude),
        ["claude-cli", "fallback"]
    );
    assert_eq!(
        commit_provider_keys(CommitMode::Ai, CommitAgent::Gemini),
        ["gemini-cli", "fallback"]
    );
    assert_eq!(
        commit_provider_keys(CommitMode::Ai, CommitAgent::Cursor),
        ["cursor-agent", "fallback"]
    );
}

#[test]
fn timestamp_mode_uses_fallback_only_whatever_the_agent() {
    for agent in CommitAgent::ALL {
        assert_eq!(
            commit_provider_keys(CommitMode::Timestamp, *agent),
            ["fallback"]
        );
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
#[test]
fn every_commit_agent_has_a_registered_provider() {
    let chain = ProviderChain::new();
    for agent in ANY_AGENT_ORDER {
        let key = agent.commit_provider_key().to_string();
        let availability = chain.check_availability(std::slice::from_ref(&key));
        assert_ne!(availability[0].1, key, "{key} has no provider in the chain");
    }
}
