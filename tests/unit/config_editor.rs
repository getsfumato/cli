use super::*;

#[test]
fn parses_toml_values_when_possible() {
    assert_eq!(parse_config_value("true"), Value::Boolean(true));
    assert_eq!(parse_config_value("4000"), Value::Integer(4000));
    assert_eq!(
        parse_config_value("sfumato-default"),
        Value::String("sfumato-default".to_string())
    );
}

#[test]
fn sets_gets_and_deletes_dotted_values() {
    let mut table = Table::new();
    set_dotted_value(
        &mut table,
        "defaults.text",
        Value::String("local-text".to_string()),
    )
    .unwrap();
    let root = Value::Table(table.clone());
    assert_eq!(
        get_dotted_value(&root, "defaults.text"),
        Some(&Value::String("local-text".to_string()))
    );
    delete_dotted_value(&mut table, "defaults.text").unwrap();
    assert!(get_dotted_value(&Value::Table(table), "defaults.text").is_none());
}

#[test]
fn renders_scalar_config_values() {
    assert_eq!(
        render_config_value(&Value::String("local-text".to_string())).unwrap(),
        "\"local-text\""
    );
}
