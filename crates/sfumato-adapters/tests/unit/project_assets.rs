use super::*;

#[test]
fn adds_lists_loads_and_removes_project_images() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let source = temporary.path().join("university-logo.png");
    std::fs::write(&source, b"not-a-decoder-test").unwrap();
    let catalog = FilesystemProjectAssetCatalog;

    let added = catalog
        .add(
            &project,
            &source,
            Some("logo"),
            Some("University logo used on title pages"),
        )
        .unwrap();
    assert_eq!(added.name, "logo");
    assert_eq!(catalog.list(&project).unwrap().len(), 1);
    assert_eq!(
        catalog.load(&project, "logo").unwrap().content_hash,
        added.content_hash
    );

    let removed = catalog.remove(&project, "logo").unwrap();
    assert!(!removed.path.exists());
    assert!(catalog.list(&project).unwrap().is_empty());
}

#[test]
fn rejects_unsafe_svg_assets() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("bad.svg");
    std::fs::write(&source, r#"<svg onload="alert(1)"></svg>"#).unwrap();
    let catalog = FilesystemProjectAssetCatalog;

    let error = catalog
        .add(temporary.path(), &source, Some("bad"), None)
        .unwrap_err();

    assert!(error.message.contains("forbidden"));
}
