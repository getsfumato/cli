use super::*;

#[test]
fn v5_round_trip_preserves_flat_options_and_nested_runtime_types() {
    let mut config = GlobalConfig::default_config();
    let profile = config.models.get_mut("local-text").unwrap();
    profile.options.text.top_p = Some(0.8);
    profile.options.image.quality = Some("high".to_string());
    profile.options.video.duration_seconds = Some(8);
    profile.options.video.resolution = Some("720p".into());

    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(&config)).unwrap();

    assert!(rendered.starts_with("schema_version = 5\n"));
    assert!(rendered.contains("temperature = 0.4"));
    assert!(rendered.contains("top_p = 0.8"));
    assert!(rendered.contains("quality = \"high\""));
    assert!(rendered.contains("video_duration_seconds = 8"));
    assert!(rendered.contains("video_resolution = \"720p\""));
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
    assert_eq!(parsed_profile.options.video.duration_seconds, Some(8));
    assert_eq!(
        parsed_profile.options.video.resolution.as_deref(),
        Some("720p")
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
fn v5_round_trip_preserves_lmstudio_connectors() {
    let mut config = GlobalConfig::default_config();
    config.connectors.insert(
        "lmstudio".to_string(),
        sfumato_core::connectors::ConnectorPreset::Lmstudio
            .into_config("lmstudio", None)
            .unwrap(),
    );
    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(&config)).unwrap();

    // The single-word spelling is pinned; `rename_all` would have emitted
    // `lm_studio` and split one kind across two names.
    assert!(rendered.contains("kind = \"lmstudio\""));
    assert!(rendered.contains("native_base_url = \"http://localhost:1234\""));

    let parsed = toml::from_str::<GlobalConfigDto>(&rendered)
        .unwrap()
        .into_domain()
        .unwrap();

    assert!(matches!(
        parsed.connectors["lmstudio"],
        ConnectorConfig::LmStudio(_)
    ));
    assert_eq!(parsed.connectors["lmstudio"].kind(), "lmstudio");
    assert!(parsed.connectors["lmstudio"].openai_compatible().is_some());
}

#[test]
fn lmstudio_connectors_accept_the_derived_snake_case_alias() {
    let document = r#"
schema_version = 5
[user]
learning_style = ["visual"]
[connectors.lmstudio]
kind = "lm_studio"
base_url = "http://localhost:1234/v1"
native_base_url = "http://localhost:1234"
[models]
[defaults]
[marp]
pdf = true
"#;

    let parsed = toml::from_str::<GlobalConfigDto>(document)
        .unwrap()
        .into_domain()
        .unwrap();

    assert!(matches!(
        parsed.connectors["lmstudio"],
        ConnectorConfig::LmStudio(_)
    ));
}

#[test]
fn v5_round_trip_preserves_anthropic_connectors() {
    let mut config = GlobalConfig::default_config();
    config.connectors.insert(
        "anthropic".to_string(),
        sfumato_core::connectors::ConnectorPreset::Anthropic
            .into_config("anthropic", None)
            .unwrap(),
    );
    let rendered = toml::to_string_pretty(&GlobalConfigDto::from_domain(&config)).unwrap();

    assert!(rendered.contains("kind = \"anthropic\""));
    assert!(rendered.contains("base_url = \"https://api.anthropic.com/v1\""));
    assert!(rendered.contains("credential = \"stored:connector/anthropic\""));

    let parsed = toml::from_str::<GlobalConfigDto>(&rendered)
        .unwrap()
        .into_domain()
        .unwrap();

    assert!(matches!(
        parsed.connectors["anthropic"],
        ConnectorConfig::Anthropic(_)
    ));
    // The Messages API is not OpenAI-compatible, so no shared transport is exposed.
    assert!(parsed.connectors["anthropic"].openai_compatible().is_none());
}

#[test]
fn anthropic_connectors_reject_native_and_process_fields() {
    let document = r#"
schema_version = 5
[user]
learning_style = ["visual"]
[connectors.anthropic]
kind = "anthropic"
base_url = "https://api.anthropic.com/v1"
native_base_url = "https://api.anthropic.com"
[models]
[defaults]
[marp]
pdf = true
"#;

    let error = toml::from_str::<GlobalConfigDto>(document)
        .unwrap()
        .into_domain()
        .expect_err("Anthropic has a single API root and no native side channel");

    assert!(
        error
            .to_string()
            .contains("cannot define executable or native_base_url")
    );
}

#[test]
fn new_connector_kinds_do_not_bump_the_schema_version() {
    // The new kinds reuse fields `ConnectorDto` already has, so a bump would
    // rewrite every user's config for no shape change and would make an older
    // binary reject a file that only contains old kinds.
    assert_eq!(CONFIG_SCHEMA_VERSION, 5);
}

#[test]
fn lmstudio_connectors_reject_an_executable() {
    let document = r#"
schema_version = 5
[user]
learning_style = ["visual"]
[connectors.lmstudio]
kind = "lmstudio"
base_url = "http://localhost:1234/v1"
native_base_url = "http://localhost:1234"
executable = "lms"
[models]
[defaults]
[marp]
pdf = true
"#;

    let error = toml::from_str::<GlobalConfigDto>(document)
        .unwrap()
        .into_domain()
        .expect_err("LM Studio is reached over HTTP, not by spawning a binary");

    assert!(error.to_string().contains("cannot define executable"));
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
        page: PageDefaults::default(),
        generation_tools: GenerationToolDefaults::default(),
        security: ProjectSecurityConfig::default(),
        marp: None,
    };
    let rendered = toml::to_string_pretty(&ProjectConfigDto::from_domain(&project)).unwrap();
    assert!(rendered.starts_with("schema_version = 5\n"));
    assert_eq!(
        toml::from_str::<ProjectConfigDto>(&rendered)
            .unwrap()
            .into_domain()
            .unwrap()
            .name,
        "university"
    );
}
