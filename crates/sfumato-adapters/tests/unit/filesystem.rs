use super::*;

#[test]
fn copies_trees_without_manifest_and_lists_regular_files() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir_all(source.join("images")).unwrap();
    fs::write(source.join("deck.md"), "# Deck").unwrap();
    fs::write(source.join("manifest.json"), "{}").unwrap();
    fs::write(source.join("images/example.png"), "image").unwrap();

    LocalWorkspaceFileSystem
        .copy_tree(&source, &destination, &["manifest.json"])
        .unwrap();
    let files = LocalWorkspaceFileSystem
        .list_files(&destination, &["manifest.json"])
        .unwrap();

    assert_eq!(files.len(), 2);
    assert!(destination.join("deck.md").is_file());
    assert!(!destination.join("manifest.json").exists());
}

#[test]
fn publishes_a_file_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("deck.pdf");
    fs::write(&source, "pdf").unwrap();

    let published = LocalWorkspaceFileSystem
        .publish_atomic(&source, &temp.path().join("published"))
        .unwrap();

    assert_eq!(fs::read_to_string(published).unwrap(), "pdf");
}
