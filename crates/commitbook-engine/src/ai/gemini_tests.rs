use super::*;
use std::ffi::OsStr;

#[test]
fn test_gemini_command_keeps_conflict_off_argv() {
    let dir = tempfile::tempdir().unwrap();
    let command = command(dir.path());
    let args: Vec<&OsStr> = command.get_args().collect();
    assert_eq!(
        args,
        vec![OsStr::new("-p"), OsStr::new(GEMINI_RESOLVE_INSTRUCTION)]
    );
    assert_eq!(command.get_program(), "gemini");
    assert_eq!(command.get_current_dir(), Some(dir.path()));
}

#[test]
fn test_gemini_resolver_key_and_name() {
    let provider = GeminiProvider;
    assert_eq!(provider.key(), "gemini");
    assert_eq!(provider.name(), "Gemini CLI");
}
