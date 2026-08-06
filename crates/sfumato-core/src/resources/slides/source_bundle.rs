//! Deterministic source budgeting before the first provider call.

use crate::{resources::build_source_index, sources::SourceDocument};

use super::excerpt;

/// Indexes the sources so the model chooses what to read.
///
/// Every stage that receives this bundle also carries the filesystem tools, so
/// the paths listed here are reachable in full. The excerpt-based bundle this
/// replaces is still built, but only for the compacted retry below, which runs
/// without tools and therefore has to carry content itself.
pub(super) fn build_source_bundle(documents: &[SourceDocument]) -> String {
    build_source_index(documents)
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
