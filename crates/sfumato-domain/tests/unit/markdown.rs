use super::*;

#[test]
fn a_separator_inside_fenced_code_is_not_a_top_level_fence() {
    // Both formats rely on this: a deck splits slides on `---` and a document
    // reads frontmatter from it, so a separator inside code must stay content.
    let markdown = "---\nkey: value\n---\n\n```yaml\n---\nnested: true\n---\n```\n";

    let located = fences(markdown);

    assert_eq!(located.len(), 2, "only the frontmatter delimiters are fences");
}

#[test]
fn heading_level_is_reported_with_its_cleaned_text() {
    assert_eq!(
        first_heading_with_level("### **Bold heading** ###\n\nbody"),
        Some((3, "Bold heading".to_owned()))
    );
}

#[test]
fn a_heading_without_a_space_is_not_a_heading() {
    // `#hashtag` is prose, not a heading, and treating it as one would split a
    // document at a word.
    assert_eq!(first_heading_with_level("#hashtag stays prose"), None);
}

#[test]
fn a_heading_inside_fenced_code_is_ignored() {
    assert_eq!(
        first_heading_with_level("```sh\n# not a heading\n```\n\n## Real\n"),
        Some((2, "Real".to_owned()))
    );
}

#[test]
fn an_unclosed_fence_is_reported_with_its_language() {
    let error = validate_fenced_code_blocks("```rust\nfn main() {}\n", "section-1")
        .expect_err("an unclosed fence is invalid");

    assert!(error.contains("unclosed `rust`"), "{error}");
    assert!(error.contains("section-1"), "{error}");
}

#[test]
fn revisions_are_stable_and_content_addressed() {
    assert_eq!(revision_for("same"), revision_for("same"));
    assert_ne!(revision_for("same"), revision_for("different"));
}
