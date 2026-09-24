use super::*;

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    tmp
}

#[test]
fn register_creates_the_id_and_the_device_file() {
    let tmp = repo();
    let id = register(tmp.path(), Some("MacBook Pro"), Auth::Ssh).unwrap();

    assert_eq!(
        this_device_id(tmp.path()).unwrap().as_deref(),
        Some(id.as_str())
    );
    let (found, device) = this_device(tmp.path()).unwrap().unwrap();
    assert_eq!(found, id);
    assert_eq!(device.name, "MacBook Pro");
    assert_eq!(device.platform, platform());
    assert_eq!(device.auth, Auth::Ssh);
    let content = fs::read_to_string(device_path(tmp.path(), &id)).unwrap();
    assert!(content.contains("name = \"MacBook Pro\""), "{content}");
}

#[test]
fn registering_again_keeps_the_existing_device() {
    let tmp = repo();
    let first = register(tmp.path(), Some("First"), Auth::Ssh).unwrap();
    let second = register(tmp.path(), Some("Second"), Auth::Pat).unwrap();

    assert_eq!(first, second);
    let (_, device) = this_device(tmp.path()).unwrap().unwrap();
    assert_eq!(device.name, "First");
    assert_eq!(device.auth, Auth::Ssh);
}

#[test]
fn register_without_a_name_uses_platform_and_short_id() {
    let tmp = repo();
    let id = register(tmp.path(), None, Auth::ExistingLocalRepo).unwrap();
    let (_, device) = this_device(tmp.path()).unwrap().unwrap();
    assert_eq!(device.name, default_name(&id));
    assert!(device.name.ends_with(&id[..4]));
}

#[test]
fn rename_changes_only_this_devices_file() {
    let tmp = repo();
    let other = "0123abcd";
    write_device(
        tmp.path(),
        other,
        &Device {
            name: "Phone".into(),
            platform: "ios".into(),
            auth: Auth::GithubApp,
        },
    )
    .unwrap();
    let id = register(tmp.path(), Some("Laptop"), Auth::Ssh).unwrap();

    rename(tmp.path(), "Work laptop").unwrap();

    let (devices, warnings) = list(tmp.path()).unwrap();
    assert!(warnings.is_empty());
    let names: Vec<_> = devices.iter().map(|d| d.device.name.as_str()).collect();
    assert_eq!(names, ["Phone", "Work laptop"]);
    assert!(devices.iter().any(|d| d.id == id && d.this_device));
    assert!(devices.iter().any(|d| d.id == other && !d.this_device));
}

#[test]
fn remove_refuses_this_device_and_deletes_others() {
    let tmp = repo();
    let id = register(tmp.path(), Some("Laptop"), Auth::Ssh).unwrap();
    let other = "0123abcd";
    write_device(
        tmp.path(),
        other,
        &Device {
            name: "Old desktop".into(),
            platform: "linux".into(),
            auth: Auth::Ssh,
        },
    )
    .unwrap();

    assert!(remove(tmp.path(), &id).is_err());
    remove(tmp.path(), other).unwrap();
    assert!(!device_path(tmp.path(), other).exists());
    assert!(remove(tmp.path(), other).is_err());
}

#[test]
fn remove_rejects_ids_that_could_escape_the_directory() {
    let tmp = repo();
    register(tmp.path(), None, Auth::Ssh).unwrap();
    assert!(remove(tmp.path(), "../config").is_err());
    assert!(remove(tmp.path(), "ABCDEF12").is_err());
}

#[test]
fn list_skips_a_malformed_file_and_reports_it() {
    let tmp = repo();
    register(tmp.path(), Some("Laptop"), Auth::Ssh).unwrap();
    fs::write(
        devices_dir(tmp.path()).join("deadbeef.toml"),
        "not = [valid",
    )
    .unwrap();

    let (devices, warnings) = list(tmp.path()).unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("deadbeef"), "{}", warnings[0]);
}

#[test]
fn names_are_trimmed_and_validated() {
    let tmp = repo();
    assert!(register(tmp.path(), Some("   "), Auth::Ssh).is_err());
    assert!(register(tmp.path(), Some("a\nb"), Auth::Ssh).is_err());
    register(tmp.path(), Some("  Desk  "), Auth::Ssh).unwrap();
    assert_eq!(this_device(tmp.path()).unwrap().unwrap().1.name, "Desk");
}
