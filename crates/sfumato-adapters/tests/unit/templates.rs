use super::*;

#[test]
fn creates_and_resolves_scaffolded_templates() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = FilesystemGenerationTemplateCatalog::new(temporary.path().to_path_buf());

    let created = catalog
        .create("lecture", TemplateKind::Slides, None)
        .unwrap();

    assert_eq!(created.manifest.name, "lecture");
    assert_eq!(created.manifest.kind, TemplateKind::Slides);
    assert!(created.source.contains(TEMPLATE_CONTENT_SLOT));
    assert_eq!(
        catalog
            .list(Some(TemplateKind::Slides))
            .unwrap()
            .entries
            .len(),
        1
    );
    assert!(
        catalog
            .list(Some(TemplateKind::Page))
            .unwrap()
            .entries
            .is_empty()
    );
}

#[test]
fn rejects_sources_without_exactly_one_content_slot() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("invalid.md");
    std::fs::write(&source, "# No slot").unwrap();
    let catalog = FilesystemGenerationTemplateCatalog::new(temporary.path().join("catalog"));

    let error = catalog
        .create("invalid", TemplateKind::Slides, Some(source))
        .unwrap_err();

    assert!(error.message.contains("exactly one"));
}

#[test]
fn rejects_loading_a_template_for_the_wrong_resource_kind() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = FilesystemGenerationTemplateCatalog::new(temporary.path().to_path_buf());
    catalog.create("lesson", TemplateKind::Page, None).unwrap();

    let error = catalog
        .load("lesson", Some(TemplateKind::Slides))
        .unwrap_err();

    assert!(error.message.contains("not slides"));
}

#[test]
fn one_damaged_package_does_not_hide_the_healthy_ones() {
    // Collecting into a single `Result` meant the first damaged package discarded
    // every package already resolved, so discovery was lost while the data was
    // fine — each healthy package stayed reachable through `show`.
    let temporary = tempfile::tempdir().unwrap();
    let catalog = FilesystemGenerationTemplateCatalog::new(temporary.path().to_path_buf());
    catalog.create("alpha", TemplateKind::Slides, None).unwrap();
    catalog.create("notes", TemplateKind::Page, None).unwrap();

    // The exact corruption the issue reproduced: an unsupported schema version.
    let manifest = temporary.path().join("notes/template.toml");
    let damaged = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("schema_version = 1", "schema_version = 2");
    std::fs::write(&manifest, damaged).unwrap();

    let listing = catalog.list(None).unwrap();

    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name, "alpha");
    assert!(!listing.is_complete());
    assert_eq!(listing.unreadable.len(), 1);
    assert_eq!(listing.unreadable[0].name, "notes");
    assert!(
        listing.unreadable[0].problem.contains("schema version"),
        "{}",
        listing.unreadable[0].problem
    );
}

#[test]
fn a_package_whose_source_is_gone_is_reported_not_fatal() {
    // The second corruption mode the issue found: manifest kept, source deleted.
    let temporary = tempfile::tempdir().unwrap();
    let catalog = FilesystemGenerationTemplateCatalog::new(temporary.path().to_path_buf());
    catalog.create("alpha", TemplateKind::Slides, None).unwrap();
    catalog.create("notes", TemplateKind::Slides, None).unwrap();
    std::fs::remove_file(temporary.path().join("notes/template.md")).unwrap();

    let listing = catalog.list(None).unwrap();

    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.unreadable.len(), 1);
}

#[test]
fn show_still_fails_hard_for_a_damaged_package() {
    // Degrading is for discovery only: a request for one specific package must
    // still report why that package cannot be used.
    let temporary = tempfile::tempdir().unwrap();
    let catalog = FilesystemGenerationTemplateCatalog::new(temporary.path().to_path_buf());
    catalog.create("notes", TemplateKind::Slides, None).unwrap();
    std::fs::remove_file(temporary.path().join("notes/template.md")).unwrap();

    assert!(catalog.load("notes", None).is_err());
}

#[test]
fn a_document_template_can_be_created_listed_and_shown() {
    // `TemplateKind::Document` was implemented everywhere except the CLI's value
    // enum, so the scaffold arm below was unreachable from outside.
    let temporary = tempfile::tempdir().unwrap();
    let catalog = FilesystemGenerationTemplateCatalog::new(temporary.path().to_path_buf());

    let created = catalog
        .create("notes", TemplateKind::Document, None)
        .unwrap();

    assert_eq!(created.manifest.kind, TemplateKind::Document);
    let listing = catalog.list(Some(TemplateKind::Document)).unwrap();
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name, "notes");
    // Inspectable without naming the kind, which the manifest already declares.
    assert_eq!(
        catalog.load("notes", None).unwrap().manifest.kind,
        TemplateKind::Document
    );
}
