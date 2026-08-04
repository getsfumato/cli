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

#[test]
fn a_thematic_break_survives_whatever_the_title_looks_like() {
    // The reported failure: `!removed` doubled as "am I still in the frontmatter?",
    // so a title that failed to match deleted every `---` in the document and every
    // `subtitle:` line, including inside a fenced code block, and left the H1 in the
    // body to duplicate the cover.
    let rendered = "---\nsubtitle: Repaso\n---\n\n# Guía\n\n## Metodología\n\nPrimer paso.\n\n---\n\nSegundo paso.\n\n## Notas\n\n```yaml\nsubtitle: ejemplo\n```\n";

    let body = strip_title_heading(rendered);

    assert_eq!(
        body.matches("\n---\n").count(),
        1,
        "thematic break lost: {body}"
    );
    assert!(
        body.contains("subtitle: ejemplo"),
        "code fence gutted: {body}"
    );
    assert!(!body.contains("# Guía"), "H1 duplicated the cover: {body}");
    assert!(
        !body.contains("subtitle: Repaso"),
        "frontmatter kept: {body}"
    );
}

#[test]
fn the_frontmatter_goes_even_when_the_title_has_stray_whitespace() {
    // One trailing space used to be enough, because the expected heading was
    // rebuilt from the title and compared against a trimmed line.
    for heading in ["# Guía ", "#  Guía", "# Guía\t"] {
        let rendered = format!("---\nsubtitle: S\n---\n\n{heading}\n\n## Uno\n\nA.\n\n---\n\nB.\n");

        let body = strip_title_heading(&rendered);

        assert!(!body.contains("subtitle: S"), "{heading:?} -> {body}");
        assert!(!body.contains("Guía"), "{heading:?} -> {body}");
        assert_eq!(body.matches("\n---\n").count(), 1, "{heading:?} -> {body}");
    }
}

#[test]
fn a_document_without_frontmatter_keeps_its_body_intact() {
    let rendered = "# Título\n\n## Uno\n\nA.\n\n---\n\nB.\n";

    let body = strip_title_heading(rendered);

    assert!(!body.contains("# Título"));
    assert_eq!(body.matches("\n---\n").count(), 1, "{body}");
    assert!(body.contains("## Uno"));
}

#[test]
fn only_the_first_level_one_heading_is_removed() {
    // The domain permits exactly one, but a `#` inside the body must not be taken
    // for the title if one ever slipped through.
    let rendered = "# Uno\n\n## Sección\n\n# Dos\n";

    let body = strip_title_heading(rendered);

    assert!(!body.contains("# Uno"));
    assert!(body.contains("# Dos"), "{body}");
}

#[test]
fn a_hash_that_is_not_a_heading_is_left_alone() {
    let rendered =
        "# Título\n\n## Uno\n\nUse #hashtag y ## en prosa.\n\n    # indentado como código\n";

    let body = strip_title_heading(rendered);

    assert!(body.contains("#hashtag"), "{body}");
    assert!(body.contains("# indentado como código"), "{body}");
}

#[test]
fn an_unterminated_frontmatter_block_does_not_swallow_the_document() {
    // Deleting to the end of the file is never the right reading of a malformed
    // document.
    let rendered = "---\nsubtitle: sin cerrar\n\n# Título\n\n## Uno\n\nA.\n";

    let body = strip_title_heading(rendered);

    assert!(body.contains("## Uno"), "{body}");
    assert!(body.contains("A."), "{body}");
}

#[test]
fn a_repeated_heading_gets_its_own_contents_entry_and_anchor() {
    // The normal case in a study document: "Conclusión" once per chapter. The
    // anchor was recomputed rather than read, and the reimplementation had no
    // duplicate counter, so every later entry linked to the first section — and
    // printed its page number, since the number comes from `target-counter()`
    // resolved against the anchor. In a printed PDF the number *is* the affordance,
    // so there is no way to tell it is wrong.
    let markdown = "# Guía\n\n## Capítulo uno\n\nA.\n\n### Conclusión\n\nB.\n\n## Capítulo dos\n\nC.\n\n### Conclusión\n\nD.\n\n## Capítulo tres\n\nE.\n\n### Conclusión\n\nF.\n";

    let assembled = assemble(markdown, setup(false, true));

    // comrak disambiguates with `-1` and `-2`; the contents must follow.
    for anchor in ["conclusión", "conclusión-1", "conclusión-2"] {
        assert!(
            assembled.html.contains(&format!("id=\"{anchor}\"")),
            "body is missing #{anchor}"
        );
        assert!(
            assembled.html.contains(&format!("href=\"#{anchor}\"")),
            "contents is missing a link to #{anchor}"
        );
    }
    // Exactly one contents entry per occurrence, so no entry points at another's
    // section. Counted inside the nav, because comrak also emits a self-link in
    // each body heading.
    assert_eq!(
        contents_of(&assembled.html)
            .matches("href=\"#conclusión\"")
            .count(),
        1,
        "the first anchor is linked more than once"
    );
}

/// Returns just the generated contents block.
fn contents_of(html: &str) -> &str {
    let start = html
        .find("<nav class=\"sfumato-contents\"")
        .expect("the contents block is present");
    let end = html[start..].find("</nav>").expect("the nav is closed") + start;
    &html[start..end]
}

#[test]
fn every_contents_entry_links_a_distinct_anchor() {
    let markdown = "# Guía\n\n## Ejemplo\n\nA.\n\n## Ejemplo\n\nB.\n\n## Ejercicios\n\nC.\n\n## Ejercicios\n\nD.\n";

    let assembled = assemble(markdown, setup(false, true));

    let contents = contents_of(&assembled.html);
    let links: Vec<&str> = contents
        .match_indices("href=\"#")
        .map(|(at, _)| {
            let rest = &contents[at + 7..];
            &rest[..rest.find('"').expect("the href is closed")]
        })
        .collect();
    let unique: std::collections::BTreeSet<&&str> = links.iter().collect();
    assert_eq!(links.len(), 4, "expected one entry per heading: {links:?}");
    assert_eq!(unique.len(), links.len(), "duplicate anchors: {links:?}");
}

#[test]
fn a_thematic_break_in_the_body_is_not_read_as_a_heading() {
    // `heading_anchors` scans the rendered HTML, where `<hr />` sits next to
    // `<h2 ...>`; matching `<h` loosely would have taken it for a heading and
    // shifted every later contents entry onto the wrong anchor.
    let markdown = "# Guía\n\n## Uno\n\nA.\n\n---\n\nB.\n\n## Dos\n\nC.\n";

    let assembled = assemble(markdown, setup(false, true));

    assert!(
        assembled.html.contains("href=\"#uno\""),
        "{}",
        assembled.html
    );
    assert!(
        assembled.html.contains("href=\"#dos\""),
        "{}",
        assembled.html
    );
}
