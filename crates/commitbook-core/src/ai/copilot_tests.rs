use super::*;

#[test]
fn test_extract_message_simple() {
    assert_eq!(extract_message("Update README.md"), "Update README.md");
}

#[test]
fn test_extract_message_with_prefix() {
    let output = "git commit -m \"Fix typo in docs\"";
    assert_eq!(extract_message(output), "Fix typo in docs");
}

#[test]
fn test_extract_message_multiline() {
    let output = "? What would you like to do?\n> Suggestion\n\nAdd new feature to parser";
    assert_eq!(extract_message(output), "Add new feature to parser");
}

#[test]
fn test_extract_message_empty() {
    assert_eq!(extract_message(""), "");
}

#[test]
fn test_extract_message_skip_headers() {
    let output = "# Some header\n? prompt\nSuggestion from copilot\nFix login bug";
    assert_eq!(extract_message(output), "Fix login bug");
}

#[test]
fn test_extract_message_single_quotes() {
    let output = "git commit -m 'Refactor auth module'";
    assert_eq!(extract_message(output), "Refactor auth module");
}

#[test]
fn test_extract_message_copilot_meta_skipped() {
    let output = "Powered by GitHub Copilot\nSuggestion:\nAdd error handling";
    assert_eq!(extract_message(output), "Add error handling");
}

#[test]
fn test_extract_message_long_truncated() {
    let long_line = "A".repeat(100);
    let result = extract_message(&long_line);
    assert!(result.len() <= 72);
}

#[test]
fn test_extract_message_new_cli_plain() {
    let output = "Add pagination to user list endpoint\n\n\nTotal usage est:       1 Premium request\nTotal duration (API):  2.9s\nTotal duration (wall): 6.3s\nTotal code changes:    0 lines added, 0 lines removed\nUsage by model:\n    claude-sonnet-4.5    11.4k input, 8 output, 0 cache read, 0 cache write (Est. 1 Premium request)";
    assert_eq!(extract_message(output), "Add pagination to user list endpoint");
}

#[test]
fn test_extract_message_new_cli_code_block() {
    let output = "Based on the diff, this change defers base file writes.\n\n**Commit message:**\n\n```\nDefer base writes until push succeeds to prevent stale state\n```\n\n\nTotal usage est:       1 Premium request";
    assert_eq!(
        extract_message(output),
        "Defer base writes until push succeeds to prevent stale state"
    );
}

#[test]
fn test_extract_message_new_cli_with_tool_output() {
    let output = "I need to see the actual changes.\n\n✓ Check git status and diff\n   $ git --no-pager status\n   ↪ 5 lines...\n\nFix race condition in sync pipeline\n\n\nTotal usage est:       1 Premium request";
    assert_eq!(
        extract_message(output),
        "Fix race condition in sync pipeline"
    );
}

#[test]
fn test_extract_message_new_cli_only_stats() {
    // Edge case: copilot returns only stats with no message
    let output = "Total usage est:       1 Premium request\nTotal duration (API):  1.0s";
    assert_eq!(extract_message(output), "");
}
