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
