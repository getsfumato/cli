use super::*;

#[test]
fn a_separator_inside_fenced_code_is_not_a_top_level_fence() {
    // Both formats rely on this: a deck splits slides on `---` and a document
    // reads frontmatter from it, so a separator inside code must stay content.
    let markdown = "---\nkey: value\n---\n\n```yaml\n---\nnested: true\n---\n```\n";

    let located = fences(markdown);

    assert_eq!(
        located.len(),
        2,
        "only the frontmatter delimiters are fences"
    );
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

#[test]
fn an_unterminated_json_fence_still_loses_its_opening_fence() {
    // The shape a response truncated at the token limit takes. One of three
    // copies chained `strip_suffix` onto `unwrap_or(value)`, so a missing closing
    // fence restored the original string with the fence still on it, and the
    // parser reported `expected value at line 1 column 1`.
    assert_eq!(strip_json_fence("```json\n{\"a\":1}"), "{\"a\":1}");
    assert_eq!(strip_json_fence("```\n{\"a\":1}"), "{\"a\":1}");
    assert_eq!(strip_json_fence("```JSON\n{\"a\":1}"), "{\"a\":1}");
}

#[test]
fn a_closed_json_fence_is_stripped_from_both_ends() {
    assert_eq!(strip_json_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
    assert_eq!(strip_json_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
}

#[test]
fn an_unfenced_response_is_returned_trimmed() {
    assert_eq!(strip_json_fence("  {\"a\":1}\n"), "{\"a\":1}");
    assert_eq!(strip_json_fence("{\"a\":1}"), "{\"a\":1}");
}

#[test]
fn leading_whitespace_before_the_fence_does_not_defeat_stripping() {
    // The copy in the domain did not trim before matching the prefix, so a
    // response that opened with a newline kept its fence.
    assert_eq!(
        strip_json_fence("\n\n```json\n{\"a\":1}\n```\n"),
        "{\"a\":1}"
    );
}

#[test]
fn a_json_string_containing_backticks_survives() {
    let value = "{\"note\":\"use ``` to fence\"}";
    assert_eq!(strip_json_fence(value), value);
}
