use super::*;

#[cfg(unix)]
#[test]
fn wait_with_timeout_drains_large_output_without_deadlock() {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    // Emit ~200 KB, far past the ~64 KB pipe buffer that would deadlock a
    // waiter that reads the pipe only after the child exits.
    let child = Command::new("sh")
        .arg("-c")
        .arg("yes 0123456789ABCDEF | head -c 200000")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let output = wait_with_timeout(child, Duration::from_secs(30)).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 200000);
}

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
    assert!(looks_like_diff_narration("The diff shows a few formatting tweaks"));
}

#[test]
fn test_diff_narration_detects_prefix_this_diff() {
    assert!(looks_like_diff_narration("This diff modifies multiple files"));
}

#[test]
fn test_diff_narration_detects_prefix_the_changes_show() {
    assert!(looks_like_diff_narration("The changes show new content added"));
}

#[test]
fn test_diff_narration_case_insensitive() {
    assert!(looks_like_diff_narration("THE STAGED CHANGES ADD X; THE UNSTAGED Y"));
}

#[test]
fn test_diff_narration_passes_normal_messages() {
    assert!(!looks_like_diff_narration("Add pagination to user list endpoint"));
    assert!(!looks_like_diff_narration("Fix race condition in sync pipeline"));
    assert!(!looks_like_diff_narration("Update README with install steps"));
}

#[test]
fn test_diff_narration_lone_word_staged_is_ok() {
    // A legitimate message about staging behavior should not be rejected
    // unless BOTH "staged" and "unstaged" appear together.
    assert!(!looks_like_diff_narration("Stage all changes before commit"));
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
    fn name(&self) -> &str { self.name }
    fn key(&self) -> &str { self.key }
    fn is_available(&self) -> bool { self.available }

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
            Box::new(MockProvider { name: "Unavail", key: "unavail", available: false, response: Some("nope") }),
            Box::new(MockProvider { name: "Avail", key: "avail", available: true, response: Some("good msg") }),
        ],
    };
    let summary = ChangesSummary { new_files: vec!["a.txt".into()], ..Default::default() };
    let (msg, provider) = chain.generate(&summary, &["unavail".into(), "avail".into()], Path::new("/tmp")).await;
    assert_eq!(msg, "good msg");
    assert_eq!(provider, "Avail");
}

#[tokio::test]
async fn test_chain_all_fail_uses_fallback() {
    let chain = ProviderChain {
        providers: vec![
            Box::new(MockProvider { name: "Bad", key: "bad", available: true, response: None }),
            Box::new(fallback::FallbackProvider),
        ],
    };
    let summary = ChangesSummary { new_files: vec!["a.txt".into()], ..Default::default() };
    let (msg, provider) = chain.generate(&summary, &["bad".into()], Path::new("/tmp")).await;
    assert_eq!(provider, "Fallback");
    assert!(!msg.is_empty());
}

#[test]
fn test_check_availability() {
    let chain = ProviderChain {
        providers: vec![
            Box::new(MockProvider { name: "Yes", key: "yes", available: true, response: None }),
            Box::new(MockProvider { name: "No", key: "no", available: false, response: None }),
        ],
    };
    let result = chain.check_availability(&["yes".into(), "no".into(), "missing".into()]);
    assert_eq!(result.len(), 3);
    assert!(result[0].2);          // "yes" is available
    assert!(!result[1].2);         // "no" is not
    assert!(!result[2].2);         // "missing" is not
}
