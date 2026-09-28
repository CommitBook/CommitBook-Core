use super::*;

#[test]
fn sync_is_supported_and_run_is_rejected() {
    assert!(matches!(
        Cli::try_parse_from(["commitbook", "sync"]).unwrap().command,
        Commands::Sync
    ));
    assert!(Cli::try_parse_from(["commitbook", "run"]).is_err());
}
