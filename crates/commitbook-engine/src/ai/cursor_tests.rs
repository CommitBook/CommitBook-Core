use super::*;
use std::ffi::OsStr;

#[test]
fn test_cursor_command_uses_print_mode_with_text_output() {
    let dir = tempfile::tempdir().unwrap();
    let command = command(dir.path());
    let args: Vec<&OsStr> = command.get_args().collect();
    assert_eq!(
        args,
        vec![
            OsStr::new("-p"),
            OsStr::new("--output-format"),
            OsStr::new("text")
        ]
    );
    assert_eq!(command.get_program(), "cursor-agent");
    assert_eq!(command.get_current_dir(), Some(dir.path()));
}

#[test]
fn test_cursor_resolver_key_and_name() {
    let provider = CursorProvider;
    assert_eq!(provider.key(), "cursor");
    assert_eq!(provider.name(), "Cursor Agent");
}
