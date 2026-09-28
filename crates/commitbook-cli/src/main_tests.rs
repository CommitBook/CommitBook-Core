use super::*;

#[test]
fn legacy_run_parses_as_sync_without_advertising_an_alias() {
    for name in ["sync", "run"] {
        let cli = Cli::try_parse_from(["commitbook", name]).unwrap();
        assert!(matches!(cli.command, Commands::Sync));
    }
    let mut command = Cli::command();
    assert!(!command.render_long_help().to_string().contains("run"));
    let sync = command.find_subcommand_mut("sync").unwrap();
    assert!(!sync.render_long_help().to_string().contains("run"));
}
