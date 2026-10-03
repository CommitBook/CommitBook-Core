use std::process::Command;

#[test]
fn aliases_share_commands_but_use_their_own_name() {
    let canonical_help = Command::new(env!("CARGO_BIN_EXE_commitbook"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(canonical_help.status.success());
    let canonical_help = String::from_utf8(canonical_help.stdout).unwrap();
    for (name, binary) in [
        ("commitbook", env!("CARGO_BIN_EXE_commitbook")),
        ("cobo", env!("CARGO_BIN_EXE_cobo")),
        ("cbook", env!("CARGO_BIN_EXE_cbook")),
    ] {
        let help = Command::new(binary).arg("--help").output().unwrap();
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains(&format!("Usage: {name}")));
        assert_eq!(
            String::from_utf8_lossy(&help.stdout)
                .replace(&format!("Usage: {name}"), "Usage: commitbook"),
            canonical_help
        );
        assert!(!Command::new(binary)
            .arg("run")
            .output()
            .unwrap()
            .status
            .success());
        let completion = Command::new(binary)
            .args(["completions", "bash"])
            .output()
            .unwrap();
        assert!(completion.status.success());
        assert!(String::from_utf8_lossy(&completion.stdout).contains(&format!("_{name}()")));
        let version = Command::new(binary).arg("--version").output().unwrap();
        assert!(version.status.success());
        assert!(String::from_utf8_lossy(&version.stdout).starts_with(name));
    }
}
