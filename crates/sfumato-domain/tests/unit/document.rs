use super::*;

use crate::parse_json_patch;

const SAMPLE: &str = "---\nsubtitle: Repaso rápido\n---\n\n# Conceptos\n\nUna introducción.\n\n## Primero\n\nCuerpo del primero.\n\n### Detalle\n\nMás detalle.\n\n## Segundo\n\nCuerpo del segundo.\n";

fn sample() -> SectionedDocument {
    SectionedDocument::from_markdown(SAMPLE).expect("the sample document is valid")
}

#[test]
fn parses_title_subtitle_preamble_and_the_section_hierarchy() {
    let document = sample();

    assert_eq!(document.title(), "Conceptos");
    assert_eq!(document.subtitle(), Some("Repaso rápido"));
    assert_eq!(document.preamble(), "Una introducción.");
    assert_eq!(document.section_count(), 3);
    assert_eq!(
        document.outline(),
        vec![(2, "Primero"), (3, "Detalle"), (2, "Segundo")]
    );
}

#[test]
fn rendering_round_trips_without_losing_content() {
    // The reviewer patches a parsed document and the workflow re-renders it, so a
    // lossy round trip would silently drop prose between two model calls.
    let document = sample();

    let rendered = document.render().expect("a valid document renders");
    let reparsed = SectionedDocument::from_markdown(&rendered).expect("rendered output reparses");

    assert_eq!(reparsed, document);
    assert_eq!(reparsed.revision(), document.revision());
}

#[test]
fn a_document_without_a_level_one_heading_is_rejected() {
    let error = SectionedDocument::from_markdown("## Only a section\n\nBody.\n")
        .expect_err("a document needs a title");

    assert!(format!("{error}").contains("level-1 heading"), "{error}");
}

#[test]
fn a_second_level_one_heading_is_rejected() {
    // Two `#` headings mean two documents in one file, and the cover would show
    // only the first of them.
    let error = SectionedDocument::from_markdown("# One\n\n## Section\n\nBody.\n\n# Two\n")
        .expect_err("a document carries exactly one title");

    assert!(
        format!("{error}").contains("exactly one level-1 heading"),
        "{error}"
    );
}

#[test]
fn a_heading_level_that_skips_a_rank_is_rejected() {
    // An outline that jumps from 2 to 4 renders a table of contents missing a
    // rank, which reads as a bug in the document rather than in the source.
    let error = SectionedDocument::from_markdown("# Title\n\n## Two\n\nBody.\n\n#### Four\n\nBody.\n")
        .expect_err("a skipped level is invalid");

    assert!(format!("{error}").contains("jumps from level 2"), "{error}");
}

#[test]
fn a_document_that_opens_below_level_two_is_rejected() {
    let error = SectionedDocument::from_markdown("# Title\n\n### Deep\n\nBody.\n")
        .expect_err("the outline needs a root");

    assert!(format!("{error}").contains("level-2"), "{error}");
}

#[test]
fn frontmatter_may_only_declare_a_subtitle() {
    // Guards against Marp directives leaking over from the deck prompts.
    let error = SectionedDocument::from_markdown("---\nmarp: true\n---\n\n# Title\n\n## S\n\nBody.\n")
        .expect_err("only subtitle is allowed");

    assert!(format!("{error}").contains("only declare `subtitle`"), "{error}");
}

#[test]
fn a_document_without_frontmatter_has_no_subtitle() {
    let document = SectionedDocument::from_markdown("# Title\n\n## Section\n\nBody.\n")
        .expect("frontmatter is optional");

    assert_eq!(document.subtitle(), None);
    assert!(document.render().expect("renders").starts_with("# Title"));
}

#[test]
fn a_patch_must_test_the_document_revision_before_changing_anything() {
    let mut document = sample();
    let patch = parse_json_patch(
        r#"[{"op":"replace","path":"/sections/section-1/markdown","value":"\n## Primero\n\nOtro cuerpo."}]"#,
    )
    .expect("the patch parses");

    let error = document
        .apply_patch(&patch)
        .expect_err("an untested patch is rejected");

    assert!(format!("{error}").contains("test `/revision`"), "{error}");
}

#[test]
fn a_patch_cannot_change_the_title() {
    // The cover and the published filename both come from the title, so letting
    // review rename it would silently fork the artifact identity.
    let mut document = sample();
    let revision = document.revision().as_str().to_owned();
    let patch = parse_json_patch(&format!(
        r#"[{{"op":"test","path":"/revision","value":"{revision}"}},{{"op":"replace","path":"/title","value":"Otro"}}]"#
    ))
    .expect("the patch parses");

    let error = document
        .apply_patch(&patch)
        .expect_err("the title is immutable");

    assert!(format!("{error}").contains("title"), "{error}");
}

#[test]
fn a_rejected_patch_leaves_the_document_untouched() {
    let mut document = sample();
    let before = document.clone();
    let revision = document.revision().as_str().to_owned();
    // Deepens a section so the outline skips a rank; validation must refuse it.
    let patch = parse_json_patch(&format!(
        r#"[{{"op":"test","path":"/revision","value":"{revision}"}},{{"op":"test","path":"/sections/section-1/revision","value":"{}"}},{{"op":"replace","path":"/sections/section-1/markdown","value":"\n##### Demasiado profundo\n\nCuerpo."}}]"#,
        document.section(&SectionId::new("section-1").unwrap()).unwrap().revision.as_str()
    ))
    .expect("the patch parses");

    assert!(document.apply_patch(&patch).is_err());
    assert_eq!(document, before);
}

#[test]
fn a_valid_patch_replaces_a_section_and_refreshes_derived_metadata() {
    let mut document = sample();
    let revision = document.revision().as_str().to_owned();
    let section = SectionId::new("section-1").unwrap();
    let section_revision = document.section(&section).unwrap().revision.as_str().to_owned();
    let patch = parse_json_patch(&format!(
        r#"[{{"op":"test","path":"/revision","value":"{revision}"}},{{"op":"test","path":"/sections/section-1/revision","value":"{section_revision}"}},{{"op":"replace","path":"/sections/section-1/markdown","value":"\n## Primero corregido\n\n- punto uno\n- punto dos"}}]"#
    ))
    .expect("the patch parses");

    let report = document.apply_patch(&patch).expect("the patch applies");

    assert_eq!(report.changed_nodes, vec!["section-1".to_owned()]);
    let updated = document.section(&section).unwrap();
    assert_eq!(updated.heading, "Primero corregido");
    assert_eq!(updated.level, 2);
    assert!(
        updated
            .elements
            .contains(&SectionElement::List { items: 2 }),
        "derived elements are refreshed: {:?}",
        updated.elements
    );
}

#[test]
fn a_patch_that_gutted_the_document_is_rejected() {
    let mut document = sample();
    let revision = document.revision().as_str().to_owned();
    let mut operations = vec![format!(
        r#"{{"op":"test","path":"/revision","value":"{revision}"}}"#
    )];
    for id in ["section-2", "section-3"] {
        let section = SectionId::new(id).unwrap();
        operations.push(format!(
            r#"{{"op":"test","path":"/sections/{id}/revision","value":"{}"}}"#,
            document.section(&section).unwrap().revision.as_str()
        ));
        operations.push(format!(r#"{{"op":"remove","path":"/sections/{id}"}}"#));
    }
    operations.push(r#"{"op":"remove","path":"/order/2"}"#.to_owned());
    operations.push(r#"{"op":"remove","path":"/order/1"}"#.to_owned());
    let patch = parse_json_patch(&format!("[{}]", operations.join(","))).expect("the patch parses");

    let error = document
        .apply_patch(&patch)
        .expect_err("a gutting patch is rejected");

    assert!(format!("{error}").contains("would reduce the document"), "{error}");
}

#[test]
fn focused_repair_replaces_one_section_by_position() {
    let mut document = sample();

    document
        .replace_section_markdown_at(2, "### Detalle reescrito\n\nTexto más corto.")
        .expect("a same-level replacement is valid");

    assert_eq!(document.outline()[1], (3, "Detalle reescrito"));
}

#[test]
fn focused_repair_cannot_break_the_hierarchy_or_empty_a_section() {
    let mut document = sample();
    let before = document.clone();

    assert!(document.replace_section_markdown_at(1, "   ").is_err());
    assert!(
        document
            .replace_section_markdown_at(1, "##### Demasiado profundo\n\nCuerpo.")
            .is_err()
    );
    assert!(document.replace_section_markdown_at(99, "## Nada").is_err());
    assert_eq!(document, before);
}

#[test]
fn the_snapshot_declares_the_document_constraints() {
    let snapshot = sample().snapshot().expect("a valid document snapshots");

    assert_eq!(snapshot.format, ReviewFormat::SectionedDocument);
    assert_eq!(snapshot.schema_version, DOCUMENT_SCHEMA_VERSION);
    for constraint in [
        ReviewConstraint::Rfc6902Only,
        ReviewConstraint::TestDocumentRevision,
        ReviewConstraint::TestSectionRevision,
        ReviewConstraint::PreserveDocumentTitle,
        ReviewConstraint::PreserveHeadingHierarchy,
    ] {
        assert!(
            snapshot.constraints.contains(&constraint),
            "{constraint:?} is missing from {:?}",
            snapshot.constraints
        );
    }
}

#[test]
fn an_unclosed_code_fence_in_a_section_is_rejected() {
    let error = SectionedDocument::from_markdown("# Title\n\n## Code\n\n```rust\nfn main() {}\n")
        .expect_err("an unclosed fence is invalid");

    assert!(format!("{error}").contains("unclosed `rust`"), "{error}");
}
