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

#[test]
fn atomically_replaces_a_published_page_tree() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("published/fourier");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("index.html"), "new page").unwrap();
    fs::write(destination.join("index.html"), "old page").unwrap();
    fs::write(destination.join("stale.png"), "stale").unwrap();

    let published = LocalWorkspaceFileSystem
        .publish_tree_atomic(&source, &destination)
        .unwrap();

    assert_eq!(
        fs::read_to_string(published.join("index.html")).unwrap(),
        "new page"
    );
    assert!(!published.join("stale.png").exists());
}

#[test]
fn a_failed_publish_names_where_the_previous_output_went() {
    // The restore error used to be discarded, so the only thing the user was
    // told was the destination name — while their previous publication sat in a
    // hidden dotfile beside it.
    let context = publish_failure_context(
        Path::new("/site/fourier"),
        PreviousPublication::Stranded {
            backup: Path::new("/site/.fourier.sfumato-backup-4321"),
            cause: "Permission denied (os error 13)".to_string(),
        },
    );

    assert!(
        context.contains("/site/.fourier.sfumato-backup-4321"),
        "{context}"
    );
    assert!(context.contains("Permission denied"), "{context}");
    assert!(context.contains("recover"), "{context}");
}

#[test]
fn a_failed_publish_says_the_previous_output_is_still_there_when_it_is() {
    let context =
        publish_failure_context(Path::new("/site/fourier"), PreviousPublication::Restored);

    assert!(context.contains("restored"), "{context}");
    assert!(context.contains("unchanged"), "{context}");
    // Nothing to recover by hand, so no backup path should be suggested.
    assert!(!context.contains("sfumato-backup"), "{context}");
}

#[test]
fn a_first_publish_failure_mentions_no_previous_output() {
    let context = publish_failure_context(Path::new("/site/fourier"), PreviousPublication::Absent);

    assert_eq!(context, "Could not atomically publish /site/fourier");
}

#[test]
fn a_successful_republish_leaves_no_backup_behind() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let published = temp.path().join("published");
    let destination = published.join("fourier");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("index.html"), "new page").unwrap();
    fs::write(destination.join("index.html"), "old page").unwrap();

    LocalWorkspaceFileSystem
        .publish_tree_atomic(&source, &destination)
        .unwrap();

    let leftovers = fs::read_dir(&published)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("sfumato-backup"))
        .collect::<Vec<_>>();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
}
