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

#[test]
fn redacts_header_names_however_they_are_spelled() {
    // `x-api-key` is the header this project itself sends, and ElevenLabs uses
    // `xi-api-key`; both survived `config show` in plaintext before, because
    // the fragment list was matched against the underscore spelling only.
    let mut value: Value = toml::from_str(
        r#"
[headers]
x-api-key = "sk-secret-1234567890"
xi-api-key = "sk-secret-1234567890"
X_API_KEY = "sk-secret-1234567890"
Cookie = "session=abc"
"#,
    )
    .unwrap();

    redact_sensitive_values(&mut value);

    for header in ["x-api-key", "xi-api-key", "X_API_KEY", "Cookie"] {
        assert_eq!(
            value["headers"][header].as_str(),
            Some("[REDACTED]"),
            "leaked {header}"
        );
    }
}

#[test]
fn keeps_showing_tuning_knobs_that_merely_contain_a_secret_word() {
    // `max_tokens` contains "token" but is an integer the user sets and reads;
    // a substring rule reported it as `[REDACTED]`.
    let mut value: Value = toml::from_str(
        r#"
[options]
max_tokens = 4000
max_tool_rounds = 8
"#,
    )
    .unwrap();

    redact_sensitive_values(&mut value);

    assert_eq!(value["options"]["max_tokens"].as_integer(), Some(4000));
    assert_eq!(value["options"]["max_tool_rounds"].as_integer(), Some(8));
}

#[test]
fn rejects_writing_a_credential_under_any_header_spelling() {
    for key in [
        "connectors.ollama.headers.api_key",
        "connectors.ollama.headers.x-api-key",
        "connectors.ollama.headers.xi-api-key",
        "connectors.ollama.headers.Authorization",
        "connectors.ollama.headers.cookie",
        "connectors.ollama.headers.X-Api-Key",
    ] {
        assert!(reject_secret_key(key).is_err(), "accepted {key}");
    }
}

#[test]
fn still_allows_writing_non_secret_keys() {
    // The write guard must not grow into a general obstruction: a credential
    // reference and a tuning knob both have to stay editable.
    for key in [
        "connectors.ollama.credential",
        "models.local-text.options.max_tokens",
        "connectors.ollama.base_url",
        "connectors.ollama.headers.HTTP-Referer",
    ] {
        assert!(reject_secret_key(key).is_ok(), "rejected {key}");
    }
}
