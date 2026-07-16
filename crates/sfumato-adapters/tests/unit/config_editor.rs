use super::*;

#[test]
fn parses_toml_values_when_possible() {
    assert_eq!(parse_config_value("true").as_bool(), Some(true));
    assert_eq!(parse_config_value("42").as_integer(), Some(42));
    assert_eq!(parse_config_value("plain").as_str(), Some("plain"));
}

#[test]
fn sets_gets_and_deletes_dotted_values() {
    let mut table = Table::new();
    set_dotted_value(&mut table, "user.name", Value::String("Alex".into())).unwrap();
    let value = Value::Table(table.clone());
    assert_eq!(
        get_dotted_value(&value, "user.name").and_then(Value::as_str),
        Some("Alex")
    );
    delete_dotted_value(&mut table, "user.name").unwrap();
    assert!(get_dotted_value(&Value::Table(table), "user.name").is_none());
}

#[test]
fn renders_scalar_config_values() {
    assert_eq!(render_config_value(&Value::Integer(4)).unwrap(), "4");
}

#[test]
fn redacts_sensitive_headers_but_keeps_secret_references() {
    let mut value: Value = toml::from_str(
        r#"
credential = "env:OPENROUTER_API_KEY"
[headers]
Authorization = "Bearer actual-secret"
HTTP-Referer = "https://example.test"
"#,
    )
    .unwrap();

    redact_sensitive_values(&mut value);

    assert_eq!(value["credential"].as_str(), Some("env:OPENROUTER_API_KEY"));
    assert_eq!(
        value["headers"]["Authorization"].as_str(),
        Some("[REDACTED]")
    );
    assert_eq!(
        value["headers"]["HTTP-Referer"].as_str(),
        Some("https://example.test")
    );
}
