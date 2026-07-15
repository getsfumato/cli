use super::*;
use crate::config::{
    CONFIG_SCHEMA_VERSION, ProjectConfig, ProjectRegistry, RegisteredProject, project_config_path,
    read_toml, write_toml,
};
use crate::themes::DEFAULT_THEME;
use std::collections::BTreeMap;

fn service(temp: &tempfile::TempDir) -> ModelService {
    ModelService::load_from(
        GlobalConfig::default_config(),
        temp.path().join("config.toml"),
        temp.path().join("projects.toml"),
    )
}

#[test]
fn adds_lists_and_shows_connector_backed_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let mut service = service(&temp);
    service
        .add(
            "local-gemma".to_string(),
            "ollama".to_string(),
            "gemma4:e2b-mlx".to_string(),
            vec!["text".to_string(), "code".to_string()],
            vec!["temperature=0.2".to_string(), "max_tokens=8000".to_string()],
        )
        .unwrap();

    let profile = service.config.models.get("local-gemma").unwrap();
    assert_eq!(profile.connector, "ollama");
    assert_eq!(profile.model, "gemma4:e2b-mlx");
    assert!(profile.capabilities.contains(&Capability::Text));
    assert_eq!(
        profile
            .options
            .get("max_tokens")
            .and_then(toml::Value::as_integer),
        Some(8000)
    );
    assert!(
        service
            .show("local-gemma")
            .unwrap()
            .contains("gemma4:e2b-mlx")
    );
    assert!(
        service
            .add(
                "missing".to_string(),
                "unknown".to_string(),
                "model".to_string(),
                vec!["text".to_string()],
                vec![],
            )
            .is_err()
    );
}

#[test]
fn assigns_user_and_project_defaults_and_protects_used_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let project_root = temp.path().join("project");
    let project_path = project_config_path(&project_root);
    write_toml(
        &project_path,
        &ProjectConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            name: "demo".to_string(),
            theme: DEFAULT_THEME.to_string(),
            publish_dir: None,
            model_defaults: Default::default(),
            model_roles: Default::default(),
            marp: None,
        },
    )
    .unwrap();
    ProjectRegistry {
        schema_version: CONFIG_SCHEMA_VERSION,
        active: Some("demo".to_string()),
        projects: BTreeMap::from([("demo".to_string(), RegisteredProject { path: project_root })]),
    }
    .save_to(&temp.path().join("projects.toml"))
    .unwrap();

    let mut service = service(&temp);
    service
        .add(
            "local-gemma".to_string(),
            "ollama".to_string(),
            "gemma4:e2b-mlx".to_string(),
            vec!["text".to_string(), "code".to_string()],
            vec![],
        )
        .unwrap();
    service.use_default("text", "local-gemma", None).unwrap();
    assert_eq!(
        service
            .config
            .defaults
            .0
            .get(&Capability::Text)
            .map(String::as_str),
        Some("local-gemma")
    );
    assert!(service.remove("local-gemma").is_err());

    service
        .use_default("code", "local-gemma", Some("demo"))
        .unwrap();
    let project: ProjectConfig = read_toml(&project_path).unwrap();
    assert_eq!(
        project
            .model_defaults
            .get(&Capability::Code)
            .map(String::as_str),
        Some("local-gemma")
    );
}

#[test]
fn assigns_and_protects_reviewer_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let mut service = service(&temp);
    service
        .add(
            "local-review".to_string(),
            "ollama".to_string(),
            "gemma3:latest".to_string(),
            vec!["text".to_string()],
            vec![],
        )
        .unwrap();

    let changed = service
        .use_default("reviewer", "local-review", None)
        .unwrap();
    assert_eq!(changed.selection, ModelSelection::Role(ModelRole::Reviewer));
    assert_eq!(
        service.config.model_roles.get(&ModelRole::Reviewer),
        Some(&"local-review".to_string())
    );
    assert!(service.remove("local-review").is_err());
    assert!(
        service
            .edit("local-review", None, None, vec!["code".to_string()], vec![],)
            .is_err()
    );
}

#[test]
fn parses_capabilities_and_typed_options() {
    assert_eq!(
        parse_capabilities(&["text".to_string(), "text".to_string()]).unwrap(),
        vec![Capability::Text]
    );
    let options = parse_options(&[
        "temperature=0.3".to_string(),
        "enabled=true".to_string(),
        "label=fast".to_string(),
    ])
    .unwrap();
    assert_eq!(
        options.get("temperature").and_then(toml::Value::as_float),
        Some(0.3)
    );
    assert_eq!(
        options.get("enabled").and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        options.get("label").and_then(toml::Value::as_str),
        Some("fast")
    );
}

#[test]
fn edits_only_supplied_profile_fields_and_merges_options() {
    let temp = tempfile::tempdir().unwrap();
    let mut service = service(&temp);
    service
        .add(
            "local-gemma".to_string(),
            "ollama".to_string(),
            "gemma4:e2b-mlx".to_string(),
            vec!["text".to_string(), "code".to_string()],
            vec!["temperature=0.4".to_string(), "max_tokens=4000".to_string()],
        )
        .unwrap();

    service
        .edit(
            "local-gemma",
            None,
            Some("gemma4:latest".to_string()),
            vec![],
            vec!["temperature=0.2".to_string()],
        )
        .unwrap();

    let profile = service.profile("local-gemma").unwrap();
    assert_eq!(profile.connector, "ollama");
    assert_eq!(profile.model, "gemma4:latest");
    assert_eq!(
        profile
            .options
            .get("temperature")
            .and_then(toml::Value::as_float),
        Some(0.2)
    );
    assert_eq!(
        profile
            .options
            .get("max_tokens")
            .and_then(toml::Value::as_integer),
        Some(4000)
    );
    assert!(
        service
            .edit("local-gemma", None, None, vec![], vec![])
            .is_err()
    );
}

#[test]
fn edit_rejects_removing_a_capability_used_by_a_default() {
    let temp = tempfile::tempdir().unwrap();
    let mut service = service(&temp);
    assert!(
        service
            .edit("local-text", None, None, vec!["code".to_string()], vec![],)
            .is_err()
    );
    assert!(
        service
            .profile("local-text")
            .unwrap()
            .capabilities
            .contains(&Capability::Text)
    );
}
