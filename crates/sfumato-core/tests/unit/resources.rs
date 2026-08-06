//! Unit tests for helpers shared by every resource workflow.

use super::*;

#[test]
fn truncated_prompt_text_says_it_was_truncated() {
    // Two of the four copies this replaces cut silently, and both fed repair
    // prompts: the model was asked to fix a document whose tail had been removed
    // with nothing indicating content was missing.
    let content = "a".repeat(100);

    let cut = excerpt(&content, 10);

    assert!(cut.starts_with(&"a".repeat(10)));
    assert!(cut.contains("truncated by sfumato"), "{cut}");
}

#[test]
fn text_that_fits_is_returned_unchanged_and_unmarked() {
    // A marker on untruncated text would be a lie the model has to interpret.
    assert_eq!(excerpt("short", 10), "short");
    assert_eq!(excerpt("exactly-ten", 11), "exactly-ten");
}

#[test]
fn truncation_counts_characters_rather_than_bytes() {
    // Accented and CJK content must not be cut mid-character.
    let content = "ñ".repeat(50);

    let cut = excerpt(&content, 10);

    assert!(cut.starts_with(&"ñ".repeat(10)));
    assert_eq!(cut.chars().filter(|c| *c == 'ñ').count(), 10);
    assert!(cut.contains("truncated by sfumato"));
}

#[test]
fn the_marker_is_the_only_thing_added() {
    let content = "abcdef";

    assert_eq!(excerpt(content, 3), "abc\n[...truncated by sfumato...]");
}

use std::path::PathBuf;

use crate::sources::SourceDocument;

fn document(path: &str, content: &str) -> SourceDocument {
    SourceDocument {
        path: PathBuf::from(path),
        content: content.to_string(),
    }
}

#[test]
fn the_index_lists_paths_instead_of_content() {
    // The whole point: pointing at a vault must not spend the context window
    // before the model has read a word.
    let documents = vec![
        document(
            "/vault/Jacobi.md",
            &format!("# Jacobi\n{}", "x".repeat(9_000)),
        ),
        document(
            "/vault/Seidel.md",
            &format!("# Seidel\n{}", "y".repeat(9_000)),
        ),
    ];

    let index = build_source_index(&documents);

    assert!(index.contains("Jacobi.md"));
    assert!(index.contains("Seidel.md"));
    assert!(!index.contains(&"x".repeat(50)), "content must stay out");
    assert!(
        index.len() < 1_000,
        "an index of two files is small: {index}"
    );
}

#[test]
fn the_index_names_the_tool_that_reaches_the_content() {
    // A listing without this reads as the whole corpus, and the model answers
    // from filenames.
    let index = build_source_index(&[document("/vault/a.md", "body")]);

    assert!(index.contains("sfumato_read_file"));
    assert!(index.contains("index, not the content"));
}

#[test]
fn files_are_grouped_under_their_directory() {
    let documents = vec![
        document("/vault/Álgebra/a.md", "body"),
        document("/vault/Álgebra/b.md", "body"),
        document("/vault/Redes/c.md", "body"),
    ];

    let index = build_source_index(&documents);

    assert!(index.contains("/vault/Álgebra/\n"));
    assert!(index.contains("/vault/Redes/\n"));
    // The directory heading is printed once per run of files, not per file.
    assert_eq!(index.matches("/vault/Álgebra/\n").count(), 1);
}

#[test]
fn sizes_are_shown_so_the_model_can_budget_what_it_reads() {
    let index = build_source_index(&[document("/vault/a.md", &"z".repeat(2_500))]);

    assert!(index.contains("2.5k chars"), "{index}");
}

#[test]
fn a_title_is_shown_only_when_it_adds_to_the_filename() {
    let repeated =
        build_source_index(&[document("/vault/Radio Espectral.md", "# Radio Espectral\n")]);
    assert!(!repeated.contains(" — Radio Espectral"), "{repeated}");

    let informative = build_source_index(&[document(
        "/vault/nota-01.md",
        "# Criterio de convergencia\n",
    )]);
    assert!(
        informative.contains(" — Criterio de convergencia"),
        "{informative}"
    );
}

#[test]
fn a_section_heading_is_not_mistaken_for_a_title() {
    // The first `##` of a note is its opening section. "El problema" tells a
    // model choosing what to read less than the filename already did.
    let index = build_source_index(&[document(
        "/vault/Condicionamiento.md",
        "Some prose.\n\n## El problema\n",
    )]);

    assert!(!index.contains("El problema"), "{index}");
}

#[test]
fn frontmatter_titles_win_over_headings() {
    let index = build_source_index(&[document(
        "/vault/nota.md",
        "---\ntitle: \"Convergencia iterativa\"\ntype: concepto\n---\n\n# Otra cosa\n",
    )]);

    assert!(index.contains(" — Convergencia iterativa"), "{index}");
}

#[test]
fn an_oversized_index_says_how_many_files_it_left_out() {
    // Silence here would read as "this is everything" to the model.
    let documents = (0..600)
        .map(|number| {
            document(
                &format!("/vault/nota-con-nombre-largo-{number:04}.md"),
                "body",
            )
        })
        .collect::<Vec<_>>();

    let index = build_source_index(&documents);

    assert!(index.contains("further file(s) omitted"), "{index}");
    assert!(index.contains("sfumato_list_directory"));
}

#[test]
fn no_sources_says_so_rather_than_rendering_an_empty_tree() {
    assert!(build_source_index(&[]).contains("No explicit source files"));
}
