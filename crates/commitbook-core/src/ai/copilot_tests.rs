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
