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
