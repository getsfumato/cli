use super::*;
use serde_json::json;

fn deck_markdown() -> &'static str {
    "---\nmarp: true\ntheme: gruvbox\npaginate: true\nmath: mathjax\n---\n\n<!-- _class: lead -->\n\n# Fourier Series\n\n---\n\n## Intuition\n\nA periodic signal is made of harmonics.\n\n---\n\n## Example\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n---\n\n## Summary\n\n- Time domain\n- Frequency domain"
}

fn patch(value: Value) -> Patch {
    serde_json::from_value(value).unwrap()
}

#[test]
fn parses_and_renders_without_losing_markdown() {
    let deck = DeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();

    assert_eq!(deck.slide_count(), 4);
    assert_eq!(snapshot.format, ReviewFormat::SlideDeck);
    assert_eq!(snapshot.revision, *deck.revision());
    assert_eq!(deck.render().unwrap(), deck_markdown());
    assert!(
        snapshot.document["slides"]["slide-3"]["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|element| element["kind"] == "mermaid")
    );
}

#[test]
fn applies_a_revision_guarded_replacement() {
    let mut deck = DeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "test", "path": "/slides/slide-2/revision", "value": snapshot.document["slides"]["slide-2"]["revision"]},
        {"op": "replace", "path": "/slides/slide-2/markdown", "value": "## Intuition\n\nA periodic signal decomposes into harmonically related sinusoids."}
    ]));

    let report = deck.apply_patch(&patch).unwrap();

    assert_eq!(report.operations, 3);
    assert_eq!(report.changed_nodes, vec!["slide-2"]);
    assert!(
        deck.render()
            .unwrap()
            .contains("harmonically related sinusoids")
    );
    deck.validate().unwrap();
}

#[test]
fn adds_a_slide_and_derives_its_metadata() {
    let mut deck = DeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "add", "path": "/slides/new-visual", "value": {"markdown": "## Visual interpretation\n\n![Spectrum](spectrum.png)"}},
        {"op": "add", "path": "/order/-", "value": "new-visual"}
    ]));

    deck.apply_patch(&patch).unwrap();
    let reviewed = deck.snapshot().unwrap();

    assert_eq!(deck.slide_count(), 5);
    assert_eq!(
        reviewed.document["slides"]["new-visual"]["heading"],
        "Visual interpretation"
    );
    assert!(
        reviewed.document["slides"]["new-visual"]["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|element| element["kind"] == "image")
    );
}

#[test]
fn rejects_unguarded_or_title_slide_mutations() {
    let mut deck = DeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let unguarded = patch(json!([
        {"op": "replace", "path": "/slides/slide-2/markdown", "value": "## Changed"}
    ]));
    assert!(
        deck.apply_patch(&unguarded)
            .unwrap_err()
            .to_string()
            .contains("/revision")
    );

    let snapshot = deck.snapshot().unwrap();
    let title = patch(json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "test", "path": "/slides/slide-1/revision", "value": snapshot.document["slides"]["slide-1"]["revision"]},
        {"op": "replace", "path": "/slides/slide-1/markdown", "value": "# Different title"}
    ]));
    assert!(
        deck.apply_patch(&title)
            .unwrap_err()
            .to_string()
            .contains("title slide")
    );
}

#[test]
fn patch_failures_leave_the_deck_unchanged() {
    let mut deck = DeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let original = deck.clone();
    let snapshot = deck.snapshot().unwrap();
    let collapsing = patch(json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "test", "path": "/slides/slide-3/revision", "value": snapshot.document["slides"]["slide-3"]["revision"]},
        {"op": "test", "path": "/slides/slide-4/revision", "value": snapshot.document["slides"]["slide-4"]["revision"]},
        {"op": "remove", "path": "/slides/slide-4"},
        {"op": "remove", "path": "/slides/slide-3"},
        {"op": "remove", "path": "/order/3"},
        {"op": "remove", "path": "/order/2"}
    ]));

    assert!(
        deck.apply_patch(&collapsing)
            .unwrap_err()
            .to_string()
            .contains("from 4 to 2")
    );
    assert_eq!(deck, original);
}

#[test]
fn rejects_moving_the_title_even_when_the_new_first_heading_matches() {
    let markdown = "---\nmarp: true\n---\n\n# Demo\n\n---\n\n# Demo";
    let mut deck = DeckDocument::from_marp(markdown, "Demo").unwrap();
    let snapshot = deck.snapshot().unwrap();
    let patch = patch(json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "move", "from": "/order/0", "path": "/order/1"}
    ]));

    assert!(
        deck.apply_patch(&patch)
            .unwrap_err()
            .to_string()
            .contains("title slide")
    );
}

#[test]
fn rejects_unclosed_code_fences_and_ignores_separators_inside_them() {
    let markdown =
        "---\nmarp: true\n---\n\n# Demo\n\n---\n\n## YAML\n\n```yaml\n---\nvalue: true\n---\n```";
    let deck = DeckDocument::from_marp(markdown, "Demo").unwrap();
    assert_eq!(deck.slide_count(), 2);
    assert_eq!(deck.render().unwrap(), markdown);

    let mut broken = DeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let snapshot = broken.snapshot().unwrap();
    let patch = patch(json!([
        {"op": "test", "path": "/revision", "value": snapshot.document["revision"]},
        {"op": "test", "path": "/slides/slide-2/revision", "value": snapshot.document["slides"]["slide-2"]["revision"]},
        {"op": "replace", "path": "/slides/slide-2/markdown", "value": "## Broken\n\n```rust\nfn main() {}"}
    ]));
    assert!(
        broken
            .apply_patch(&patch)
            .unwrap_err()
            .to_string()
            .contains("unclosed `rust`")
    );
}

#[test]
fn focused_layout_replacement_uses_one_based_validated_slide_positions() {
    let mut deck = DeckDocument::from_marp(deck_markdown(), "Fourier Series").unwrap();
    let original_revision = deck.revision().clone();

    deck.replace_slide_markdown_at(2, "## Improved intuition\n\nOne compact idea.")
        .unwrap();

    assert_eq!(
        deck.slide_at(2).unwrap().1.heading.as_deref(),
        Some("Improved intuition")
    );
    assert_ne!(deck.revision(), &original_revision);
    assert!(deck.replace_slide_markdown_at(1, "# Replaced").is_err());
    assert!(deck.replace_slide_markdown_at(99, "## Missing").is_err());
    assert!(
        deck.replace_slide_markdown_at(2, "## Split\n\n---\n\n## Extra")
            .is_err()
    );

    deck.replace_slide_fragment_at(2, "## Part one\n\nShort.\n\n---\n\n## Part two\n\nShort.")
        .unwrap();
    assert_eq!(deck.slide_count(), 5);
    assert_eq!(
        deck.slide_at(2).unwrap().1.heading.as_deref(),
        Some("Part one")
    );
    assert_eq!(
        deck.slide_at(3).unwrap().1.heading.as_deref(),
        Some("Part two")
    );
}
