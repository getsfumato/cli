//! End-to-end check that a document actually paginates and prints.
//!
//! Ignored by default because it drives a real browser; run it explicitly with
//! `cargo test -p sfumato-adapters --test document_pipeline -- --ignored`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use sfumato_adapters::{documents::PagedDocumentAssembler, renderers::PagedDocumentCliRenderer};
use sfumato_core::{
    generation::{DocumentPageSetup, DocumentPageSize},
    operation::OperationContext,
    renderers::{
        DocumentAssembler, DocumentAssemblyRequest, DocumentRenderRequest, DocumentRenderer,
        SectionedDocument,
    },
    themes::{ThemeAdapters, ThemeManifest, ThemePackage, ThemeTokens},
};

fn theme() -> ThemePackage {
    ThemePackage {
        root: PathBuf::from("/themes/probe"),
        manifest: ThemeManifest {
            schema_version: 1,
            name: "probe".into(),
            description: "probe".into(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([("primary".to_owned(), "#315c8c".to_owned())]),
                fonts: BTreeMap::from([("body".to_owned(), "Georgia, serif".to_owned())]),
            },
            adapters: ThemeAdapters {
                marp_css: PathBuf::from("marp/theme.css"),
                html: None,
                document: None,
            },
        },
    }
}

fn long_markdown() -> String {
    let mut markdown = String::from(
        "---\nsubtitle: Prueba de paginado\n---\n\n# Conceptos de repaso\n\nIntroducción al material.\n",
    );
    for section in 1..=4 {
        markdown.push_str(&format!("\n## Sección {section}\n\n"));
        for paragraph in 1..=6 {
            markdown.push_str(&format!(
                "Párrafo {paragraph} de la sección {section}. {}\n\n",
                "Texto de relleno suficientemente largo para forzar varios saltos de página. "
                    .repeat(4)
            ));
        }
        markdown.push_str(&format!("### Detalle {section}\n\n| columna | valor |\n| --- | --- |\n| uno | 1 |\n| dos | 2 |\n\n"));
    }
    markdown
}

#[tokio::test]
#[ignore = "drives the real Paged.js CLI and a headless browser"]
async fn a_document_paginates_numbers_its_pages_and_prints_a_pdf() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let document = SectionedDocument::from_markdown(&long_markdown()).expect("valid document");
    let theme = theme();
    let assembled = PagedDocumentAssembler
        .assemble(DocumentAssemblyRequest {
            document: &document,
            theme: &theme,
            setup: DocumentPageSetup {
                page_size: DocumentPageSize::A4,
                table_of_contents: true,
                cover: true,
            },
            project: "probe",
            revision_date: "2026-07-29",
            allowed_assets: &[],
        })
        .expect("assembly succeeds");

    let root = workspace.path();
    std::fs::write(root.join("document.html"), &assembled.html).expect("write html");

    let renderer = PagedDocumentCliRenderer;
    let operation = OperationContext::detached();
    let request = || DocumentRenderRequest {
        workspace_root: root,
        document: Path::new("document.html"),
        output: Path::new("document.pdf"),
        setup: DocumentPageSetup {
            page_size: DocumentPageSize::A4,
            table_of_contents: true,
            cover: true,
        },
    };

    let issues = renderer
        .inspect_format(request(), &operation)
        .await
        .expect("inspection succeeds");
    println!("format issues: {issues:?}");
    // The measurement scratch files exist only to carry the probe.
    for leftover in ["document.paginated.html", "document.paginated.measure.html"] {
        assert!(
            !root.join(leftover).exists(),
            "{leftover} is cleaned up after measuring"
        );
    }

    let first = renderer
        .render_pdf(request(), &operation)
        .await
        .expect("printing succeeds");
    assert!(
        first.pages >= 3,
        "cover, contents and body span pages, got {}",
        first.pages
    );
    let bytes = std::fs::read(root.join("document.pdf")).expect("read pdf");
    assert!(bytes.starts_with(b"%PDF-"), "the output is a real PDF");

    // The whole reason for the CLI: the same input has to produce the same
    // pagination every time, which driving a browser directly did not.
    let second = renderer
        .render_pdf(request(), &operation)
        .await
        .expect("printing succeeds again");
    assert_eq!(
        first.pages, second.pages,
        "pagination is deterministic across runs"
    );
}

/// A document carrying both defects at once: repeated headings and a thematic
/// break beside a fenced `subtitle:` example.
fn repeated_headings_markdown() -> String {
    let mut markdown =
        String::from("---\nsubtitle: Repaso\n---\n\n# Guía práctica\n\nIntroducción.\n");
    for chapter in 1..=3 {
        markdown.push_str(&format!("\n## Capítulo {chapter}\n\n"));
        markdown.push_str(&format!(
            "{}\n\n",
            "Relleno para empujar cada capítulo a su propia página. ".repeat(40)
        ));
        markdown.push_str("---\n\nTexto tras la línea divisoria.\n\n");
        markdown.push_str("```yaml\nsubtitle: ejemplo\n```\n\n");
        markdown.push_str("### Conclusión\n\nCierre del capítulo.\n\n");
    }
    markdown
}

#[tokio::test]
#[ignore = "drives the real Paged.js CLI and a headless browser"]
async fn repeated_headings_get_distinct_pages_and_the_body_survives() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let source = repeated_headings_markdown();
    let document = SectionedDocument::from_markdown(&source).expect("valid document");
    let theme = theme();
    let setup = DocumentPageSetup {
        page_size: DocumentPageSize::A4,
        table_of_contents: true,
        cover: true,
    };
    let assembled = PagedDocumentAssembler
        .assemble(DocumentAssemblyRequest {
            document: &document,
            theme: &theme,
            setup,
            project: "probe",
            revision_date: "2026-07-29",
            allowed_assets: &[],
        })
        .expect("assembly succeeds");

    // The body kept what a bad title used to delete.
    assert_eq!(
        assembled.html.matches("<hr").count(),
        3,
        "a thematic break was deleted"
    );
    assert_eq!(
        assembled.html.matches("subtitle: ejemplo").count(),
        3,
        "a fenced example was gutted"
    );
    assert!(
        !assembled.html.contains("subtitle: Repaso"),
        "the frontmatter leaked into the body"
    );

    // Each repeated heading has its own anchor, so each contents entry resolves
    // its own page number.
    for anchor in ["conclusión", "conclusión-1", "conclusión-2"] {
        assert!(
            assembled.html.contains(&format!("href=\"#{anchor}\"")),
            "the contents does not link #{anchor}"
        );
    }

    let root = workspace.path();
    std::fs::write(root.join("document.html"), &assembled.html).expect("write html");
    let renderer = PagedDocumentCliRenderer;
    let operation = OperationContext::detached();
    let issues = renderer
        .inspect_format(
            DocumentRenderRequest {
                workspace_root: root,
                document: Path::new("document.html"),
                output: Path::new("document.pdf"),
                setup,
            },
            &operation,
        )
        .await
        .expect("inspection succeeds");

    // Paged.js resolves each entry's page number from its anchor via
    // `target-counter()`, so distinct anchors are what make the numbers distinct;
    // this confirms the document those anchors are in still paginates.
    assert!(
        issues.is_empty(),
        "the document did not paginate cleanly: {issues:?}"
    );
}
