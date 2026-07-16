use super::*;

const DECK: &str = r#"---
marp: true
theme: gruvbox
paginate: true
math: mathjax
---

# Fourier Series

---

## Definition

Old explanation.

---

## Example

Keep this example.
"#;

fn replacement_patch(document: &SlideDeckDocument, replacement: &str) -> String {
    let snapshot = document.snapshot().unwrap();
    let slide_revision = snapshot.document["slides"]["slide-2"]["revision"]
        .as_str()
        .unwrap();
    serde_json::json!([
        { "op": "test", "path": "/revision", "value": snapshot.revision },
        { "op": "test", "path": "/slides/slide-2/revision", "value": slide_revision },
        { "op": "replace", "path": "/slides/slide-2/markdown", "value": replacement }
    ])
    .to_string()
}

#[test]
fn applies_content_patch_without_regenerating_deck_metadata() {
    let document = SlideDeckDocument::from_marp(DECK, "Fourier Series").unwrap();
    let patch = replacement_patch(
        &document,
        "## Definition\n\nA periodic signal is represented by harmonics.",
    );

    let (edited, report) = apply_edit_response(&document, &patch).unwrap();

    assert_eq!(report.changed_nodes, vec!["slide-2"]);
    assert_eq!(report.operations, 3);
    assert!(edited.starts_with("---\nmarp: true\ntheme: gruvbox"));
    assert!(edited.contains("# Fourier Series"));
    assert!(edited.contains("A periodic signal is represented by harmonics."));
    assert!(edited.contains("Keep this example."));
    assert!(!edited.contains("Old explanation."));
}

#[test]
fn rejects_structural_edit_operations() {
    let document = SlideDeckDocument::from_marp(DECK, "Fourier Series").unwrap();
    let snapshot = document.snapshot().unwrap();
    let patch = serde_json::json!([
        { "op": "test", "path": "/revision", "value": snapshot.revision },
        { "op": "remove", "path": "/slides/slide-2" }
    ])
    .to_string();

    let error = apply_edit_response(&document, &patch).unwrap_err();

    assert!(error.to_string().contains("may only replace"));
}

#[test]
fn reads_the_theme_from_existing_frontmatter() {
    assert_eq!(deck_theme_name(DECK).unwrap(), "gruvbox");
}
