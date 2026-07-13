use super::*;

fn deck_markdown() -> &'static str {
    "---\nmarp: true\ntheme: gruvbox\npaginate: true\nmath: mathjax\n---\n\n<!-- _class: lead -->\n\n# Fourier Series\n\n---\n\n## Intuition\n\nA periodic signal is made of harmonics.\n\n---\n\n## Example\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n---\n\n## Summary\n\n- Time domain\n- Frequency domain"
}

fn patch(value: Value) -> Patch {
    serde_json::from_value(value).unwrap()
}

#[test]
fn parses_and_renders_a_structured_deck_without_losing_markdown() {
    let deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let rendered = deck.render().unwrap();

    assert_eq!(deck.slide_count(), 4);
    assert_eq!(snapshot.format, ReviewFormat::SlideDeck);
    assert!(
        snapshot.document["slides"]["slide-3"]["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|element| element["kind"] == "mermaid")
    );
    assert_eq!(rendered, deck_markdown());
}

#[test]
fn applies_a_revision_guarded_slide_replacement() {
    let mut deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let deck_revision = snapshot.document["revision"].as_str().unwrap();
    let slide_revision = snapshot.document["slides"]["slide-2"]["revision"]
        .as_str()
        .unwrap();
    let patch = patch(serde_json::json!([
        {"op": "test", "path": "/revision", "value": deck_revision},
        {"op": "test", "path": "/slides/slide-2/revision", "value": slide_revision},
        {"op": "replace", "path": "/slides/slide-2/markdown", "value": "## Intuition\n\nA periodic signal decomposes into harmonically related sinusoids."}
    ]));

    let report = deck.apply_patch(&patch).unwrap();
    let rendered = deck.render().unwrap();

    assert_eq!(report.operations, 3);
    assert_eq!(report.changed_nodes, vec!["slide-2"]);
    assert!(rendered.contains("harmonically related sinusoids"));
    assert!(rendered.contains("## Example"));
    assert!(!rendered.contains("A periodic signal is made of harmonics"));
}

#[test]
fn adds_a_new_slide_with_derived_metadata() {
    let mut deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(serde_json::json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "add", "path": "/slides/new-visual", "value": {"markdown": "## Visual interpretation\n\nThe spectrum shows each harmonic."}},
        {"op": "add", "path": "/order/-", "value": "new-visual"}
    ]));

    let report = deck.apply_patch(&patch).unwrap();
    let reviewed = deck.snapshot().unwrap();

    assert_eq!(deck.slide_count(), 5);
    assert_eq!(report.changed_nodes, vec!["new-visual"]);
    assert_eq!(
        reviewed.document["slides"]["new-visual"]["heading"],
        "Visual interpretation"
    );
}

#[test]
fn rejects_changes_without_revision_tests() {
    let mut deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let patch = patch(serde_json::json!([
        {"op": "replace", "path": "/slides/slide-2/markdown", "value": "## Changed"}
    ]));

    let error = deck.apply_patch(&patch).unwrap_err();

    assert!(error.to_string().contains("must test `/revision`"));
}

#[test]
fn rejects_modifications_to_the_title_slide() {
    let mut deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(serde_json::json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "test", "path": "/slides/slide-1/revision", "value": snapshot.document["slides"]["slide-1"]["revision"]},
        {"op": "replace", "path": "/slides/slide-1/markdown", "value": "# Different title"}
    ]));

    let error = deck.apply_patch(&patch).unwrap_err();

    assert!(error.to_string().contains("title slide"));
}

#[test]
fn rejects_root_document_replacement() {
    let mut deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(serde_json::json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "replace", "path": "", "value": {}}
    ]));

    let error = deck.apply_patch(&patch).unwrap_err();

    assert!(error.to_string().contains("only replace slide Markdown"));
}

#[test]
fn rejects_a_patch_that_collapses_most_of_the_deck() {
    let mut deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let original = deck.render().unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(serde_json::json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "test", "path": "/slides/slide-3/revision", "value": snapshot.document["slides"]["slide-3"]["revision"]},
        {"op": "test", "path": "/slides/slide-4/revision", "value": snapshot.document["slides"]["slide-4"]["revision"]},
        {"op": "remove", "path": "/slides/slide-4"},
        {"op": "remove", "path": "/slides/slide-3"},
        {"op": "remove", "path": "/order/3"},
        {"op": "remove", "path": "/order/2"}
    ]));

    let error = deck.apply_patch(&patch).unwrap_err();

    assert!(error.to_string().contains("reduce the deck from 4 to 2"));
    assert_eq!(deck.render().unwrap(), original);
}

#[test]
fn rejects_an_unclosed_mermaid_fence_and_preserves_the_deck() {
    let mut deck = SlideDeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let original = deck.render().unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(serde_json::json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "test", "path": "/slides/slide-2/revision", "value": snapshot.document["slides"]["slide-2"]["revision"]},
        {"op": "replace", "path": "/slides/slide-2/markdown", "value": "## Broken diagram\n\n```mermaid\nflowchart LR\n  A --> B"}
    ]));

    let error = deck.apply_patch(&patch).unwrap_err();

    assert!(error.to_string().contains("unclosed `mermaid`"));
    assert_eq!(deck.render().unwrap(), original);
}

#[test]
fn ignores_slide_separators_inside_code_fences() {
    let markdown =
        "---\nmarp: true\n---\n\n# Demo\n\n---\n\n## YAML\n\n```yaml\n---\nvalue: true\n---\n```";
    let deck = SlideDeckDocument::from_marp(markdown, "Demo").unwrap();

    assert_eq!(deck.slide_count(), 2);
    assert_eq!(deck.render().unwrap(), markdown);
}
