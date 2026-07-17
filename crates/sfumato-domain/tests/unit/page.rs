use json_patch::Patch;

use super::*;

fn page() -> PageDocument {
    PageDocument::new(
        "Fourier Explorer",
        "<section><h1>Fourier Explorer</h1></section>",
        "#sfumato-page { display: grid; }",
        "document.querySelector('h1').dataset.ready = 'true';",
    )
    .unwrap()
}

#[test]
fn page_snapshot_is_html_and_revision_guarded() {
    let snapshot = page().snapshot().unwrap();
    assert_eq!(snapshot.format, ReviewFormat::Html);
    assert!(
        snapshot
            .constraints
            .contains(&ReviewConstraint::TestDocumentRevision)
    );
}

#[test]
fn semantic_review_can_replace_content_fields() {
    let mut page = page();
    let patch: Patch = serde_json::from_value(serde_json::json!([
        {"op": "test", "path": "/revision", "value": page.revision()},
        {"op": "replace", "path": "/body_html", "value": "<main>Updated</main>"}
    ]))
    .unwrap();
    let report = page.apply_patch(&patch).unwrap();
    assert_eq!(report.changed_nodes, vec!["body_html"]);
    assert_eq!(page.body_html(), "<main>Updated</main>");
}

#[test]
fn browser_repair_cannot_replace_title() {
    let mut page = page();
    let patch: Patch = serde_json::from_value(serde_json::json!([
        {"op": "test", "path": "/revision", "value": page.revision()},
        {"op": "replace", "path": "/title", "value": "Different"}
    ]))
    .unwrap();
    assert!(page.apply_browser_repair_patch(&patch).is_err());
}

#[test]
fn failed_patch_leaves_page_unchanged() {
    let mut page = page();
    let original = page.clone();
    let patch: Patch = serde_json::from_value(serde_json::json!([
        {"op": "replace", "path": "/css", "value": "body {}"}
    ]))
    .unwrap();
    assert!(page.apply_patch(&patch).is_err());
    assert_eq!(page, original);
}
