use super::*;

#[test]
fn test_resolve_repo_path_with_value() {
    let result = resolve_repo_path(Some(Path::new("/tmp"))).unwrap();
    assert!(result.to_string_lossy().contains("tmp"));
}

#[test]
fn test_resolve_repo_path_none_uses_cwd() {
    let result = resolve_repo_path(None).unwrap();
    assert_eq!(result, std::env::current_dir().unwrap());
}
