use super::*;

#[test]
fn test_parse_response_simple() {
    assert_eq!(
        parse_response("Update README.md").unwrap(),
        "Update README.md"
    );
}

#[test]
fn test_parse_response_strips_outer_fence() {
    let output = "```markdown\n# Title\nBody\n```";
    assert_eq!(parse_response(output).unwrap(), "# Title\nBody");
}

#[test]
fn test_parse_response_preserves_multiline_content() {
    let output = "First line\n\nSecond line";
    assert_eq!(parse_response(output).unwrap(), output);
}

#[test]
fn test_parse_response_rejects_empty() {
    assert!(parse_response("").is_err());
}

#[test]
fn test_parse_response_rejects_usage_diagnostics() {
    let output = "Resolved content\nTotal usage est: 1 premium request";
    assert!(parse_response(output).is_err());
}

#[test]
fn test_parse_response_rejects_tool_diagnostics() {
    let output = "Resolved content\n✓ Read file\n$ sed -n 1,20p note.md\n↪ 20 lines";
    assert!(parse_response(output).is_err());
}

#[test]
fn test_copilot_command_uses_argument_boundary_and_silent_mode() {
    let command = command(Path::new("/tmp/repo"), "write a message");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert_eq!(
        args,
        [
            "copilot",
            "--",
            "-p",
            "write a message",
            "--silent",
            "--no-color",
            "--no-custom-instructions",
        ]
    );
}
