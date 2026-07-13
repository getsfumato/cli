use super::*;

#[test]
fn parses_plain_and_fenced_json_patches() {
    let plain = parse_json_patch("[]").unwrap();
    let fenced = parse_json_patch("```json\n[]\n```").unwrap();

    assert!(plain.0.is_empty());
    assert!(fenced.0.is_empty());
}

#[test]
fn rejects_a_markdown_deck_instead_of_a_json_patch() {
    let error = parse_json_patch("# Reviewed deck").unwrap_err();

    assert!(error.to_string().contains("RFC 6902 JSON Patch"));
}
