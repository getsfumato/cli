use super::*;

#[test]
fn publishes_pages_under_the_visible_sfumato_namespace() {
    assert_eq!(
        page_publish_destination(Path::new("/vault/course"), "fourier-series"),
        PathBuf::from("/vault/course/_sfumato/pages/fourier-series")
    );
}

#[test]
fn creates_an_obsidian_index_for_the_generated_page() {
    let index = obsidian_page_index("Fourier \"Series\"", "university", "rev-123");

    assert!(index.contains("sfumato: generated"));
    assert!(index.contains("resource: page"));
    assert!(index.contains(r#"title: "Fourier \"Series\"""#));
    assert!(index.contains("project: \"university\""));
    assert!(index.contains("revision: \"rev-123\""));
    assert!(index.contains("[Open interactive page](./index.html)"));
}

#[test]
fn page_draft_fills_a_template_without_repeating_the_document_shell() {
    let template = GenerationTemplate {
        root: PathBuf::from("/templates/explorer"),
        manifest: crate::templates::GenerationTemplateManifest {
            schema_version: crate::templates::TEMPLATE_SCHEMA_VERSION,
            name: "explorer".into(),
            kind: crate::templates::TemplateKind::Page,
            description: "Explorer shell".into(),
            source: PathBuf::from("template.html"),
        },
        source: format!(
            "<article class=\"lesson\"><nav>Contents</nav>{}</article>",
            crate::templates::TEMPLATE_CONTENT_SLOT
        ),
    };
    let response = r#"{"title":"Fourier Explorer","body_html":"<section><h1>Series</h1></section>","css":"","javascript":""}"#;

    let page = parse_page_document(response, None, Some(&template)).unwrap();

    assert_eq!(
        page.body_html(),
        "<article class=\"lesson\"><nav>Contents</nav><section><h1>Series</h1></section></article>"
    );
}
