/// Build the append-both conflict content for a section where both
/// local and remote have changes.
///
/// Format:
/// ```text
/// ...local version...
///
/// ---
/// Merged remote version from sync on {timestamp}
/// ---
///
/// ...remote version...
/// ```
pub fn build_append_both(local_content: &str, remote_content: &str, timestamp: &str) -> String {
    let mut output = String::new();
    output.push_str(local_content);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
    output.push_str("---\n");
    output.push_str(&format!("Merged remote version from sync on {timestamp}\n"));
    output.push_str("---\n");
    output.push('\n');
    output.push_str(remote_content);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod conflict_builder_tests {
    use super::*;

    #[test]
    fn test_append_both_basic() {
        let result = build_append_both(
            "Local notes here",
            "Remote notes here",
            "2026-04-06T20:00:00Z",
        );
        assert!(result.contains("Local notes here"));
        assert!(result.contains("Remote notes here"));
        assert!(result.contains("---"));
        assert!(result.contains("Merged remote version from sync on 2026-04-06T20:00:00Z"));
    }

    #[test]
    fn test_append_both_multiline() {
        let result = build_append_both(
            "Line 1\nLine 2",
            "Remote 1\nRemote 2",
            "2026-04-06T20:00:00Z",
        );
        assert!(result.contains("Line 1\nLine 2"));
        assert!(result.contains("Remote 1\nRemote 2"));
    }
}
