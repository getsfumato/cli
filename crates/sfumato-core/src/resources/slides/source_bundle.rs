//! Deterministic source budgeting before the first provider call.

use crate::sources::SourceDocument;

use super::{MAX_SOURCE_BUNDLE_CHARS, excerpt};

pub(super) fn build_source_bundle(documents: &[SourceDocument]) -> String {
    if documents.is_empty() {
        return "No explicit source files were supplied.".to_string();
    }
    let per_document = (MAX_SOURCE_BUNDLE_CHARS / documents.len().max(1)).clamp(500, 6_000);
    let bundle = documents
        .iter()
        .map(|document| {
            let excerpt = excerpt(&document.content, per_document);
            format!(
                "\n--- SOURCE: {} ---\n{}\n",
                document.path.display(),
                excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    excerpt(&bundle, MAX_SOURCE_BUNDLE_CHARS)
}

pub(super) fn build_compact_source_bundle(
    documents: &[SourceDocument],
    max_chars: usize,
) -> String {
    if documents.is_empty() {
        return "No explicit source files were supplied.".to_string();
    }
    let index = documents
        .iter()
        .map(|document| format!("- {}", document.path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let index_chars = index.chars().count();
    let remaining = max_chars.saturating_sub(index_chars + 32);
    let per_document = (remaining / documents.len().max(1)).clamp(200, 1_200);
    let excerpts = documents
        .iter()
        .map(|document| {
            format!(
                "\n--- {} ---\n{}",
                document.path.display(),
                excerpt(&document.content, per_document)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    excerpt(
        &format!("Source index:\n{index}\n\nDistributed excerpts:{excerpts}"),
        max_chars,
    )
}
