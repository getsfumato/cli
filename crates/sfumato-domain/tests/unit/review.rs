use super::*;

#[test]
fn parses_plain_and_fenced_json_patch_arrays() {
    let plain = parse_json_patch(r#"[{"op":"test","path":"/revision","value":"1"}]"#).unwrap();
    let fenced = parse_json_patch(
        "```json\n[{\"op\":\"test\",\"path\":\"/revision\",\"value\":\"1\"}]\n```",
    )
    .unwrap();

    assert_eq!(plain, fenced);
    assert_eq!(plain.0.len(), 1);
}

#[test]
fn rejects_non_patch_json_and_oversized_patches() {
    assert!(parse_json_patch(r#"{"op":"test"}"#).is_err());

    let source = format!(
        "[{}]",
        std::iter::repeat_n(r#"{"op":"test","path":"/revision","value":"1"}"#, 33)
            .collect::<Vec<_>>()
            .join(",")
    );
    let error = parse_json_patch(&source).unwrap_err();
    assert!(matches!(error, ReviewError::TooManyOperations { .. }));
}
