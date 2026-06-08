use super::*;

#[test]
fn parses_toml_values_when_possible() {
    assert_eq!(parse_config_value("true"), Value::Boolean(true));
    assert_eq!(parse_config_value("4000"), Value::Integer(4000));
    assert_eq!(parse_config_value("0.4"), Value::Float(0.4));
    assert_eq!(
        parse_config_value("[\"visual\", \"practice\"]"),
        Value::Array(vec![
            Value::String("visual".to_string()),
            Value::String("practice".to_string())
        ])
    );
    assert_eq!(
        parse_config_value("sfumato-default"),
        Value::String("sfumato-default".to_string())
    );
}

#[test]
fn sets_and_gets_dotted_values() {
    let mut table = Table::new();
    set_dotted_value(
        &mut table,
        "inference.model",
        Value::String("llama3.2".to_string()),
    )
    .unwrap();

    let root = Value::Table(table);
    assert_eq!(
        get_dotted_value(&root, "inference.model"),
        Some(&Value::String("llama3.2".to_string()))
    );
}

#[test]
fn deletes_dotted_values() {
    let mut table = Table::new();
    set_dotted_value(&mut table, "marp.pdf", Value::Boolean(true)).unwrap();
    delete_dotted_value(&mut table, "marp.pdf").unwrap();

    let root = Value::Table(table);
    assert!(get_dotted_value(&root, "marp.pdf").is_none());
}

#[test]
fn effective_scope_is_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let service = ConfigService {
        user_config_path: temp.path().join("user.toml"),
        project_config_path: temp.path().join("project.toml"),
    };

    assert!(
        service
            .set(ConfigScope::Effective, "user.theme", "dark")
            .is_err()
    );
    assert!(
        service
            .delete(ConfigScope::Effective, "user.theme")
            .is_err()
    );
}

#[test]
fn renders_scalar_config_values() {
    assert_eq!(
        render_config_value(&Value::String("sfumato-default".to_string())).unwrap(),
        "\"sfumato-default\""
    );
}
