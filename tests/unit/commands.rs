use super::*;

#[test]
fn parses_repeatable_capability_model_overrides() {
    let parsed = parse_model_overrides(&[
        "text=local-text".to_string(),
        "image=cloud-image".to_string(),
    ])
    .unwrap();
    assert_eq!(parsed.get(&Capability::Text).unwrap(), "local-text");
    assert_eq!(parsed.get(&Capability::Image).unwrap(), "cloud-image");
}

#[test]
fn rejects_invalid_model_override() {
    assert!(parse_model_overrides(&["local-text".to_string()]).is_err());
}
