use super::*;
use std::fs;

#[test]
fn parse_reports_filter_drivers_only() {
    let content = "\
# comment filter=ignored
*.md text eol=lf
secrets/** filter=git-crypt diff=git-crypt
*.psd filter=lfs diff=lfs merge=lfs -text
*.bin -filter
plain.txt !filter
[attr]crypt filter=git-crypt
";
    let found = parse_filters(".gitattributes", content);
    let filters: Vec<(&str, &str)> = found
        .iter()
        .map(|f| (f.pattern.as_str(), f.filter.as_str()))
        .collect();
    assert_eq!(
        filters,
        [
            ("secrets/**", "git-crypt"),
            ("*.psd", "lfs"),
            ("[attr]crypt", "git-crypt"),
        ]
    );
    assert_eq!(
        found[0].to_string(),
        "`filter=git-crypt` for `secrets/**` in .gitattributes"
    );
}

fn temp_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    tmp
}

#[test]
fn a_repository_without_filters_is_supported() {
    let tmp = temp_repo();
    fs::write(tmp.path().join(".gitattributes"), "*.md text eol=lf\n").unwrap();
    assert!(unsupported_filters(tmp.path()).unwrap().is_empty());
    ensure_filters_supported(tmp.path()).unwrap();
}

#[test]
fn finds_filters_in_root_nested_and_info_attributes() {
    let tmp = temp_repo();
    fs::write(tmp.path().join(".gitattributes"), "*.psd filter=lfs\n").unwrap();
    fs::create_dir_all(tmp.path().join("private")).unwrap();
    fs::write(
        tmp.path().join("private/.gitattributes"),
        "* filter=git-crypt\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join(".git/info/attributes"),
        "*.key filter=sops\n",
    )
    .unwrap();

    let mut found: Vec<(String, String)> = unsupported_filters(tmp.path())
        .unwrap()
        .into_iter()
        .map(|f| (f.source, f.filter))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            (".git/info/attributes".to_string(), "sops".to_string()),
            (".gitattributes".to_string(), "lfs".to_string()),
            (
                "private/.gitattributes".to_string(),
                "git-crypt".to_string()
            ),
        ]
    );

    let error = ensure_filters_supported(tmp.path()).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("filter=git-crypt"), "{message}");
    assert!(message.contains("unfiltered"), "{message}");
}

#[test]
fn finds_filters_in_a_tracked_unmodified_attributes_file() {
    let tmp = temp_repo();
    fs::write(tmp.path().join(".gitattributes"), "*.psd filter=lfs\n").unwrap();
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_path(std::path::Path::new(".gitattributes"))
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "attrs", &tree, &[])
        .unwrap();

    let found = unsupported_filters(tmp.path()).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].filter, "lfs");
}
