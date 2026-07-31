use super::*;

use sfumato_core::{
    generation::{DocumentPageSetup, DocumentPageSize},
    themes::{ThemeAdapters, ThemeManifest, ThemeTokens},
};
use std::{collections::BTreeMap, path::PathBuf};

const MARKDOWN: &str = "---\nsubtitle: Material de repaso\n---\n\n# Conceptos\n\nIntroducción.\n\n## Primero\n\nCuerpo con **énfasis**.\n\n### Detalle\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n## Segundo\n\nMás cuerpo.\n";

fn theme() -> ThemePackage {
    ThemePackage {
        root: PathBuf::from("/themes/test"),
        manifest: ThemeManifest {
            schema_version: 1,
            name: "test".into(),
            description: "test theme".into(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([
                    ("primary".to_owned(), "#123456".to_owned()),
                    ("text".to_owned(), "#111111".to_owned()),
                ]),
                fonts: BTreeMap::from([("body".to_owned(), "Georgia, serif".to_owned())]),
            },
            // No document adapter: exercises the bundled fallback stylesheet.
            adapters: ThemeAdapters {
                marp_css: PathBuf::from("marp/theme.css"),
                html: None,
                document: None,
            },
        },
    }
}

fn setup(cover: bool, table_of_contents: bool) -> DocumentPageSetup {
    DocumentPageSetup {
        page_size: DocumentPageSize::A4,
        table_of_contents,
        cover,
    }
}

fn assemble(markdown: &str, setup: DocumentPageSetup) -> AssembledDocument {
    let document = SectionedDocument::from_markdown(markdown).expect("the document is valid");
    let theme = theme();
    PagedDocumentAssembler
        .assemble(DocumentAssemblyRequest {
            document: &document,
            theme: &theme,
            setup,
            project: "university",
            revision_date: "2026-07-29",
            allowed_assets: &[],
        })
        .expect("assembly succeeds")
}

#[test]
fn assembles_a_cover_a_contents_list_and_the_body() {
    let assembled = assemble(MARKDOWN, setup(true, true));

    assert!(assembled.html.contains("class=\"sfumato-cover\""));
    assert!(assembled.html.contains("Material de repaso"));
    assert!(assembled.html.contains("university"));
    assert!(assembled.html.contains("2026-07-29"));
    assert!(assembled.html.contains("role=\"doc-toc\""));
    assert!(assembled.html.contains("class=\"sfumato-document\""));
    assert!(assembled.html.contains("<table>"));
    assert!(assembled.html.contains("<strong>énfasis</strong>"));
}

#[test]
fn the_title_appears_on_the_cover_and_not_twice_in_the_body() {
    // The cover and the running header both carry the title; leaving the level-1
    // heading in the flow would print it a third time on page one of the body.
    let assembled = assemble(MARKDOWN, setup(true, true));

    let body = assembled
        .html
        .split_once("class=\"sfumato-document\"")
        .expect("the body exists")
        .1;

    assert!(!body.contains("<h1"), "the body keeps no level-1 heading");
    assert!(assembled.html.contains("sfumato-cover-title"));
}

#[test]
fn no_cover_emits_neither_the_page_nor_the_block() {
    let assembled = assemble(MARKDOWN, setup(false, true));

    assert!(!assembled.html.contains("sfumato-cover\""));
    assert!(assembled.html.contains("role=\"doc-toc\""));
}

#[test]
fn no_contents_emits_no_navigation() {
    let assembled = assemble(MARKDOWN, setup(true, false));

    assert!(!assembled.html.contains("role=\"doc-toc\""));
    assert!(assembled.html.contains("sfumato-cover"));
}

#[test]
fn a_document_without_a_subtitle_leaves_no_empty_line_on_the_cover() {
    let assembled = assemble(
        "# Solo título\n\n## Sección\n\nCuerpo.\n",
        setup(true, false),
    );

    // Matched against the emitted element, not the class name: the bundled
    // stylesheet is inlined into the same document and always mentions the class.
    assert!(
        assembled
            .html
            .contains("<h1 class=\"sfumato-cover-title\">Solo título</h1>")
    );
    assert!(
        !assembled
            .html
            .contains("<p class=\"sfumato-cover-subtitle\">")
    );
}

#[test]
fn contents_entries_link_to_the_anchors_comrak_generates() {
    // A contents entry whose href does not match its heading's id renders as a
    // dead link and, worse, a blank page number.
    let assembled = assemble(MARKDOWN, setup(false, true));

    for anchor in ["primero", "detalle", "segundo"] {
        assert!(
            assembled.html.contains(&format!("href=\"#{anchor}\"")),
            "the contents list links #{anchor}"
        );
        assert!(
            assembled.html.contains(&format!("id=\"{anchor}\"")),
            "the body carries the #{anchor} anchor"
        );
    }
}

#[test]
fn contents_entries_carry_their_outline_level() {
    let assembled = assemble(MARKDOWN, setup(false, true));

    assert!(assembled.html.contains("data-level=\"2\""));
    assert!(assembled.html.contains("data-level=\"3\""));
}

#[test]
fn the_page_setup_reaches_the_stylesheet() {
    let a4 = assemble(MARKDOWN, setup(false, false));
    assert!(a4.html.contains("size: 210mm 297mm"));

    let letter = assemble(
        MARKDOWN,
        DocumentPageSetup {
            page_size: DocumentPageSize::Letter,
            ..setup(false, false)
        },
    );
    assert!(letter.html.contains("size: 8.5in 11in"));
}

#[test]
fn a_theme_without_a_document_adapter_falls_back_to_bundled_print_css() {
    let assembled = assemble(MARKDOWN, setup(false, false));

    // The fallback still carries the theme's own tokens rather than hard-coded
    // colours, so a pre-existing theme prints in its own palette.
    assert!(assembled.html.contains("--sfumato-primary: #123456"));
    assert!(
        assembled
            .html
            .contains("--sfumato-body-font: Georgia, serif")
    );
    assert!(assembled.html.contains("@bottom-center"));
}

#[test]
fn the_assembled_document_embeds_no_paginator() {
    // The renderer's CLI injects its own paginator; a second one embedded here
    // would paginate the document twice.
    let assembled = assemble(MARKDOWN, setup(false, false));

    assert!(!assembled.html.contains("pagedjs"));
    assert!(
        !assembled
            .runtimes
            .iter()
            .any(|runtime| runtime.id == "pagedjs")
    );
}

#[test]
fn math_survives_markdown_and_reaches_mathjax_with_its_delimiters() {
    // CommonMark reads `\(` as an escaped parenthesis, so math has to be parsed
    // and re-delimited or the formula silently loses its markers.
    let assembled = assemble(
        "# Título\n\n## Fórmulas\n\nInline $E = mc^2$ y en bloque:\n\n$$\\int_0^1 x\\,dx$$\n",
        setup(false, false),
    );

    assert!(
        assembled.html.contains("\\(E = mc^2\\)"),
        "inline math keeps its delimiters"
    );
    assert!(assembled.html.contains("\\int_0^1 x"));
    assert!(!assembled.html.contains("data-math-style"));
    assert!(
        assembled
            .runtimes
            .iter()
            .any(|runtime| runtime.id == "mathjax"),
        "the math runtime is embedded only when math is present"
    );
}

#[test]
fn a_document_without_math_embeds_no_math_runtime() {
    let assembled = assemble(MARKDOWN, setup(false, false));

    assert!(
        !assembled
            .runtimes
            .iter()
            .any(|runtime| runtime.id == "mathjax")
    );
}

#[test]
fn a_referenced_image_that_was_never_generated_is_rejected() {
    // A missing image prints as a broken box in a PDF nobody can repair after the
    // fact, so it fails at assembly instead.
    let document = SectionedDocument::from_markdown(
        "# Título\n\n## Sección\n\n![diagrama](images/missing.svg)\n",
    )
    .expect("the document is valid");
    let theme = theme();

    let error = PagedDocumentAssembler
        .assemble(DocumentAssemblyRequest {
            document: &document,
            theme: &theme,
            setup: setup(false, false),
            project: "university",
            revision_date: "2026-07-29",
            allowed_assets: &[],
        })
        .expect_err("an ungenerated asset is rejected");

    assert!(
        format!("{error}").contains("not a generated asset"),
        "{error}"
    );
}

#[test]
fn a_generated_image_is_accepted() {
    let document = SectionedDocument::from_markdown(
        "# Título\n\n## Sección\n\n![diagrama](diagrams/diagram-abc.svg)\n",
    )
    .expect("the document is valid");
    let theme = theme();

    let assembled = PagedDocumentAssembler
        .assemble(DocumentAssemblyRequest {
            document: &document,
            theme: &theme,
            setup: setup(false, false),
            project: "university",
            revision_date: "2026-07-29",
            allowed_assets: &[PathBuf::from("/staging/diagrams/diagram-abc.svg")],
        })
        .expect("a generated asset is accepted");

    assert!(assembled.html.contains("diagrams/diagram-abc.svg"));
}

#[test]
fn assembly_is_deterministic() {
    // The revision date is passed in rather than read from the clock, so the same
    // document and revision must produce identical bytes.
    assert_eq!(
        assemble(MARKDOWN, setup(true, true)).html,
        assemble(MARKDOWN, setup(true, true)).html
    );
}
