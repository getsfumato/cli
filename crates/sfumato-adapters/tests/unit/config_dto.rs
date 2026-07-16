use super::*;

#[test]
fn v4_round_trip_preserves_flat_options_and_nested_runtime_types() {
    let mut config = GlobalConfig::default_config();
    let profile = config.models.get_mut("local-text").unwrap();
    profile.options.text.top_p = Some(0.8);
    profile.options.image.quality = Some("high".to_string());

    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(&config)).unwrap();

    assert!(rendered.starts_with("schema_version = 4\n"));
    assert!(rendered.contains("temperature = 0.4"));
    assert!(rendered.contains("top_p = 0.8"));
    assert!(rendered.contains("quality = \"high\""));
    assert!(!rendered.contains("[models.local-text.options.text]"));
    assert!(!rendered.contains("[models.local-text.options.image]"));

    let persisted: GlobalConfigDto = toml::from_str(&rendered).unwrap();
    let parsed = persisted.into_domain().unwrap();
    let parsed_profile = parsed.models.get("local-text").unwrap();
    assert_eq!(parsed_profile.options.text.top_p, Some(0.8));
    assert_eq!(
        parsed_profile.options.image.quality.as_deref(),
        Some("high")
    );
}

#[test]
fn v4_global_config_without_model_roles_uses_an_empty_map() {
    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(
        &GlobalConfig::default_config(),
    ))
    .unwrap()
    .replace("[model_roles]\n", "");

    let persisted: GlobalConfigDto = toml::from_str(&rendered).unwrap();
    let parsed = persisted.into_domain().unwrap();

    assert!(parsed.model_roles.is_empty());
}

#[test]
fn project_and_registry_versions_are_owned_by_persistence_dtos() {
    let registry = ProjectRegistry::default();
    let persisted = ProjectRegistryDto::from_domain(&registry);
    assert_eq!(persisted.schema_version, CONFIG_SCHEMA_VERSION);

    let project = ProjectConfig {
        name: "university".to_string(),
        theme: "sfumato-default".to_string(),
        publish_dir: None,
        model_defaults: BTreeMap::new(),
        model_roles: BTreeMap::new(),
        marp: None,
    };
    let rendered = toml::to_string_pretty(&ProjectConfigDto::from_domain(&project)).unwrap();
    assert!(rendered.starts_with("schema_version = 4\n"));
    assert_eq!(
        toml::from_str::<ProjectConfigDto>(&rendered)
            .unwrap()
            .into_domain()
            .unwrap()
            .name,
        "university"
    );
}
