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
fn legacy_v4_connectors_without_kind_remain_openai_compatible() {
    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(
        &GlobalConfig::default_config(),
    ))
    .unwrap()
    .replace("kind = \"openai_compatible\"\n", "");

    let parsed = toml::from_str::<GlobalConfigDto>(&rendered)
        .unwrap()
        .into_domain()
        .unwrap();

    assert!(parsed.connectors["ollama"].openai_compatible().is_some());
}

#[test]
fn v4_round_trip_preserves_codex_app_server_connectors() {
    let mut config = GlobalConfig::default_config();
    config.connectors.insert(
        "codex".to_string(),
        ConnectorConfig::CodexAppServer(CodexAppServerConnectorConfig {
            executable: PathBuf::from("codex"),
        }),
    );

    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(&config)).unwrap();
    assert!(rendered.contains("kind = \"codex_app_server\""));
    assert!(rendered.contains("executable = \"codex\""));

    let parsed = toml::from_str::<GlobalConfigDto>(&rendered)
        .unwrap()
        .into_domain()
        .unwrap();
    assert!(matches!(
        parsed.connectors["codex"],
        ConnectorConfig::CodexAppServer(_)
    ));
}

#[test]
fn v4_round_trip_preserves_native_openrouter_and_ollama_composition() {
    let config = GlobalConfig::default_config();
    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(&config)).unwrap();

    assert!(rendered.contains("kind = \"openrouter\""));
    assert!(rendered.contains("kind = \"ollama\""));
    assert!(rendered.contains("native_base_url = \"http://localhost:11434\""));

    let parsed = toml::from_str::<GlobalConfigDto>(&rendered)
        .unwrap()
        .into_domain()
        .unwrap();
    assert!(matches!(
        parsed.connectors["openrouter"],
        ConnectorConfig::OpenRouter(_)
    ));
    assert!(matches!(
        parsed.connectors["ollama"],
        ConnectorConfig::Ollama(_)
    ));
    assert!(
        parsed.connectors["openrouter"]
            .openai_compatible()
            .is_some()
    );
    assert!(parsed.connectors["ollama"].openai_compatible().is_some());
}

#[test]
fn legacy_codex_cli_kind_loads_as_app_server_and_normalizes_on_write() {
    let mut config = GlobalConfig::default_config();
    config.connectors.insert(
        "codex".to_string(),
        ConnectorConfig::CodexAppServer(CodexAppServerConnectorConfig {
            executable: PathBuf::from("codex"),
        }),
    );
    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(&config))
        .unwrap()
        .replace("kind = \"codex_app_server\"", "kind = \"codex_cli\"");

    let parsed = toml::from_str::<GlobalConfigDto>(&rendered)
        .unwrap()
        .into_domain()
        .unwrap();

    assert!(matches!(
        parsed.connectors["codex"],
        ConnectorConfig::CodexAppServer(_)
    ));
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
        plugins: Vec::new(),
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
