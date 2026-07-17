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
    assert_eq!(catalog.list(Some(TemplateKind::Slides)).unwrap().len(), 1);
    assert!(catalog.list(Some(TemplateKind::Page)).unwrap().is_empty());
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

    let error = catalog.load("lesson", TemplateKind::Slides).unwrap_err();

    assert!(error.message.contains("not slides"));
}
