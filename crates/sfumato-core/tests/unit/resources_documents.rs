use super::*;

use crate::{
    generation::DocumentFormatIssueKind,
    themes::{DocumentThemeAdapter, ThemeAdapters, ThemeManifest, ThemeTokens},
};

fn theme(document: Option<DocumentThemeAdapter>) -> ThemePackage {
    ThemePackage {
        root: PathBuf::from("/themes/test"),
        manifest: ThemeManifest {
            schema_version: 1,
            name: "test".into(),
            description: "test".into(),
            tokens: ThemeTokens {
                colors: BTreeMap::new(),
                fonts: BTreeMap::new(),
            },
            adapters: ThemeAdapters {
                marp_css: PathBuf::from("marp/theme.css"),
                html: None,
                document,
            },
        },
    }
}

fn adapter(
    page_size: Option<&str>,
    toc: Option<bool>,
    cover: Option<bool>,
) -> DocumentThemeAdapter {
    DocumentThemeAdapter {
        css: PathBuf::from("document/print.css"),
        page_size: page_size.map(ToOwned::to_owned),
        table_of_contents: toc,
        cover,
        cover_image: None,
    }
}

fn issue(
    section: usize,
    page: usize,
    overflow: u32,
    kind: DocumentFormatIssueKind,
) -> DocumentFormatIssue {
    DocumentFormatIssue {
        page,
        section,
        heading: format!("Section {section}"),
        kind,
        overflow_px: overflow,
        element: "table".into(),
    }
}

#[test]
fn a_theme_without_a_document_adapter_still_resolves_a_page_setup() {
    // Every theme installed before documents existed has to keep working.
    let setup = resolve_page_setup(&theme(None), None, None, None).expect("resolves");

    assert_eq!(setup.page_size, DocumentPageSize::A4);
    assert!(setup.table_of_contents);
    assert!(setup.cover);
}

#[test]
fn the_theme_supplies_the_defaults() {
    let theme = theme(Some(adapter(Some("letter"), Some(false), Some(false))));

    let setup = resolve_page_setup(&theme, None, None, None).expect("resolves");

    assert_eq!(setup.page_size, DocumentPageSize::Letter);
    assert!(!setup.table_of_contents);
    assert!(!setup.cover);
}

#[test]
fn an_explicit_flag_overrides_the_theme() {
    let theme = theme(Some(adapter(Some("letter"), Some(false), Some(false))));

    let setup = resolve_page_setup(&theme, Some(DocumentPageSize::A4), Some(true), Some(true))
        .expect("resolves");

    assert_eq!(setup.page_size, DocumentPageSize::A4);
    assert!(setup.table_of_contents);
    assert!(setup.cover);
}

#[test]
fn an_unsupported_theme_page_size_fails_loudly() {
    // A typo in a theme has to surface as an error, not silently print A4.
    let theme = theme(Some(adapter(Some("a3"), None, None)));

    let error = resolve_page_setup(&theme, None, None, None).expect_err("a3 is unsupported");

    assert!(
        format!("{error}").contains("Unsupported page size"),
        "{error}"
    );
}

#[test]
fn the_cover_date_comes_from_the_revision_not_the_clock() {
    // The revision stamps nanoseconds since the epoch, so the same revision
    // always yields the same date and the PDF stays reproducible.
    let revision = format!("rev-{:x}", 1_754_000_000_u128 * 1_000_000_000);

    assert_eq!(revision_date(&revision), "2025-07-31");
    assert_eq!(revision_date(&revision), revision_date(&revision));
    // The epoch and a leap-year boundary, so the calendar arithmetic is pinned
    // rather than just self-consistent.
    assert_eq!(revision_date("rev-0"), "1970-01-01");
    assert_eq!(
        revision_date(&format!("rev-{:x}", 1_709_164_800_u128 * 1_000_000_000)),
        "2024-02-29"
    );
}

#[test]
fn an_unparseable_revision_falls_back_to_itself() {
    assert_eq!(revision_date("dry-run"), "dry-run");
}

#[test]
fn a_diagram_is_embedded_by_width_rather_than_slide_height() {
    // A slide constrains a diagram by height to fit its fixed box; a document
    // constrains it by the width of the text column.
    let markdown = document_image_markdown("diagram-abc");

    assert_eq!(markdown, "![Diagram](diagrams/diagram-abc.svg)");
    assert!(!markdown.contains("height:"));
}

#[test]
fn format_repair_takes_the_worst_defect_first() {
    // One oversized element reports on several pages, so clearing the largest
    // offender usually clears the rest with it.
    let assessment = FormatAssessment::new(vec![
        issue(1, 2, 12, DocumentFormatIssueKind::OrphanedHeading),
        issue(3, 5, 90, DocumentFormatIssueKind::OverflowsTextColumn),
        issue(2, 3, 40, DocumentFormatIssueKind::TallerThanPage),
    ]);

    assert_eq!(assessment.next_issue().expect("a defect").section, 3);
}

#[test]
fn a_repair_is_kept_only_when_it_improves_the_document() {
    let mut assessment = FormatAssessment::new(vec![
        issue(1, 2, 50, DocumentFormatIssueKind::OverflowsTextColumn),
        issue(2, 3, 50, DocumentFormatIssueKind::OverflowsTextColumn),
    ]);

    // Fewer defects is an improvement.
    assert!(assessment.accept_if_improved(vec![issue(
        1,
        2,
        50,
        DocumentFormatIssueKind::OverflowsTextColumn
    )]));
    // Trading one defect for another of the same count and severity is churn.
    assert!(!assessment.accept_if_improved(vec![issue(
        4,
        6,
        50,
        DocumentFormatIssueKind::OrphanedHeading
    )]));
    // A strictly worse set is rejected too.
    assert!(!assessment.accept_if_improved(vec![
        issue(1, 2, 50, DocumentFormatIssueKind::OverflowsTextColumn),
        issue(2, 3, 10, DocumentFormatIssueKind::OrphanedHeading),
    ]));
}

#[test]
fn a_section_that_cannot_be_repaired_is_abandoned_so_the_loop_terminates() {
    let mut assessment = FormatAssessment::new(vec![issue(
        1,
        2,
        50,
        DocumentFormatIssueKind::OverflowsTextColumn,
    )]);

    assessment.give_up_on(1);

    assert!(assessment.next_issue().is_none());
    // The defect still survives into the report; abandoning it is not fixing it.
    assert_eq!(assessment.into_issues().len(), 1);
}

#[test]
fn a_fenced_answer_is_unwrapped() {
    // Models wrap Markdown in a fence even when told not to; the alternative to
    // stripping it is a document whose first section is a code block.
    assert_eq!(
        strip_markdown_fence("```markdown\n# Title\n\n## Section\n```"),
        "# Title\n\n## Section"
    );
    assert_eq!(strip_markdown_fence("  # Title\n"), "# Title");
}

#[test]
fn a_title_that_cannot_name_a_file_is_rejected() {
    assert!(validate_title("   ".into()).is_err());
    assert!(validate_title("!!!".into()).is_err());
    assert_eq!(
        validate_title("  Conceptos de repaso  ".into()).expect("valid"),
        "Conceptos de repaso"
    );
}

#[test]
fn parsing_rejects_a_document_whose_title_is_not_the_requested_one() {
    // The requested title names the artifact, so a drafter that renames it would
    // publish under a filename the caller never asked for.
    let error = parse_document("# Otro\n\n## Sección\n\nCuerpo.\n", Some("Pedido"))
        .expect_err("the title must match");

    assert!(format!("{error}").contains("was requested"), "{error}");
}

#[test]
fn parsing_accepts_a_document_whose_title_matches() {
    let document = parse_document("# Pedido\n\n## Sección\n\nCuerpo.\n", Some("Pedido"))
        .expect("the title matches");

    assert_eq!(document.title(), "Pedido");
}
