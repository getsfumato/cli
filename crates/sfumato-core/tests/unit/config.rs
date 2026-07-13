use super::*;

fn effective_config() -> EffectiveConfig {
    let global = GlobalConfig::default_config();
    EffectiveConfig {
        user: global.user,
        project_name: "university".to_string(),
        project_root: PathBuf::from("/tmp/university"),
        output_dir: PathBuf::from("Resources/Sfumato"),
        theme: "sfumato-default".to_string(),
        connectors: global.connectors,
        models: global.models,
        model_defaults: global.defaults.0,
        model_roles: global.model_roles,
        marp: global.marp,
    }
}

#[test]
fn resolves_default_model_by_capability() {
    let config = effective_config();
    let (name, profile) = config.resolve_model(Capability::Text).unwrap();
    assert_eq!(name, "local-text");
    assert!(profile.capabilities.contains(&Capability::Text));
}

#[test]
fn rejects_profile_without_required_capability() {
    let mut config = effective_config();
    config
        .model_defaults
        .insert(Capability::Image, "local-text".to_string());
    assert!(config.resolve_model(Capability::Image).is_err());
}

#[test]
fn registry_selects_active_or_requested_project() {
    let registry = ProjectRegistry {
        schema_version: CONFIG_SCHEMA_VERSION,
        active: Some("one".to_string()),
        projects: BTreeMap::from([
            (
                "one".to_string(),
                RegisteredProject {
                    path: PathBuf::from("/tmp/one"),
                },
            ),
            (
                "two".to_string(),
                RegisteredProject {
                    path: PathBuf::from("/tmp/two"),
                },
            ),
        ]),
    };
    assert_eq!(registry.selected(None).unwrap().0, "one");
    assert_eq!(registry.selected(Some("two")).unwrap().0, "two");
}

#[test]
fn command_theme_wins_over_project_theme() {
    assert_eq!(
        resolve_theme_name("sfumato-default", Some("gruvbox".to_string())),
        "gruvbox"
    );
    assert_eq!(
        resolve_theme_name("sfumato-default", None),
        "sfumato-default"
    );
}

#[test]
fn migrates_global_project_and_registry_configs_with_backups() {
    let temp = tempfile::tempdir().unwrap();
    let global_path = temp.path().join("config.toml");
    let global = GlobalConfig::default_config();
    let mut global_value = toml::Value::try_from(global).unwrap();
    global_value
        .as_table_mut()
        .unwrap()
        .remove("schema_version");
    global_value["user"].as_table_mut().unwrap().insert(
        "theme".to_string(),
        toml::Value::String("legacy".to_string()),
    );
    global_value["marp"].as_table_mut().unwrap().insert(
        "theme".to_string(),
        toml::Value::String("default".to_string()),
    );
    global_value.as_table_mut().unwrap().insert(
        "diagrams".to_string(),
        toml::Value::Table(toml::Table::from_iter([(
            "renderer".to_string(),
            toml::Value::String("mermaid-cli".to_string()),
        )])),
    );
    fs::write(&global_path, toml::to_string_pretty(&global_value).unwrap()).unwrap();
    migrate_global_config(&global_path).unwrap();
    let migrated_global: GlobalConfig = read_toml(&global_path).unwrap();
    assert_eq!(migrated_global.schema_version, CONFIG_SCHEMA_VERSION);
    assert!(
        read_toml_value(&global_path)
            .unwrap()
            .get("diagrams")
            .is_none()
    );
    assert!(PathBuf::from(format!("{}.bak", global_path.display())).exists());

    let project_path = temp.path().join("project.toml");
    fs::write(
        &project_path,
        "name = \"demo\"\noutput_dir = \"Resources/Sfumato\"\n\n[marp]\ntheme = \"default\"\npdf = true\n",
    )
    .unwrap();
    let project = load_project_config(&project_path, "legacy").unwrap();
    assert_eq!(project.theme, "legacy");
    assert!(project.marp.unwrap().pdf);
    assert!(PathBuf::from(format!("{}.bak", project_path.display())).exists());

    let registry_path = temp.path().join("projects.toml");
    fs::write(&registry_path, "active = \"demo\"\n\n[projects]\n").unwrap();
    let registry = ProjectRegistry::load_from(&registry_path).unwrap();
    assert_eq!(registry.schema_version, CONFIG_SCHEMA_VERSION);
    assert!(PathBuf::from(format!("{}.bak", registry_path.display())).exists());
}

#[test]
fn new_global_config_round_trips_through_toml() {
    let config = GlobalConfig::default_config();
    let rendered = toml::to_string_pretty(&config).unwrap();
    let parsed: GlobalConfig = toml::from_str(&rendered).unwrap();
    assert!(parsed.models.contains_key("local-text"));
    assert_eq!(
        parsed.defaults.0.get(&Capability::Text).map(String::as_str),
        Some("local-text")
    );
}

#[test]
fn command_model_default_wins_over_project_and_user() {
    let merged = merge_model_defaults(
        BTreeMap::from([(Capability::Text, "user".to_string())]),
        BTreeMap::from([(Capability::Text, "project".to_string())]),
        BTreeMap::from([(Capability::Text, "command".to_string())]),
    );
    assert_eq!(merged.get(&Capability::Text).unwrap(), "command");
}

#[test]
fn reviewer_role_resolves_explicit_profile_or_draft_fallback() {
    let mut config = effective_config();
    let (fallback_name, _) = config.resolve_model_role(ModelRole::Reviewer).unwrap();
    assert_eq!(fallback_name, "local-text");

    config
        .model_roles
        .insert(ModelRole::Reviewer, "cloud-text".to_string());
    let (reviewer_name, reviewer) = config.resolve_model_role(ModelRole::Reviewer).unwrap();
    assert_eq!(reviewer_name, "cloud-text");
    assert_eq!(reviewer.connector, "openrouter");
}

#[test]
fn reviewer_role_rejects_missing_connector_or_text_capability() {
    let mut config = effective_config();
    config.models.insert(
        "invalid-reviewer".to_string(),
        ModelProfile {
            connector: "missing".to_string(),
            model: "model".to_string(),
            capabilities: vec![Capability::Text],
            options: Default::default(),
        },
    );
    config
        .model_roles
        .insert(ModelRole::Reviewer, "invalid-reviewer".to_string());
    assert!(config.resolve_model_role(ModelRole::Reviewer).is_err());

    config.models.get_mut("invalid-reviewer").unwrap().connector = "ollama".to_string();
    config
        .models
        .get_mut("invalid-reviewer")
        .unwrap()
        .capabilities = vec![Capability::Code];
    assert!(config.resolve_model_role(ModelRole::Reviewer).is_err());
}

#[test]
fn reviewer_override_wins_over_project_and_user_roles() {
    let merged = merge_model_roles(
        BTreeMap::from([(ModelRole::Reviewer, "user".to_string())]),
        BTreeMap::from([(ModelRole::Reviewer, "project".to_string())]),
        Some("command".to_string()),
    );
    assert_eq!(merged.get(&ModelRole::Reviewer).unwrap(), "command");
}

#[test]
fn existing_config_without_model_roles_still_loads() {
    let rendered = toml::to_string_pretty(&GlobalConfig::default_config()).unwrap();
    let rendered = rendered.replace("[model_roles]\n", "");
    let parsed: GlobalConfig = toml::from_str(&rendered).unwrap();
    assert!(parsed.model_roles.is_empty());
}

#[test]
fn migrates_hybrid_v2_config_with_legacy_inference_and_providers() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let old = r#"
schema_version = 2

[user]
learning_style = ["visual"]

[inference]
provider = "ollama"
model = "llama3.2"
temperature = 0.5
max_tokens = 4000

[providers.ollama]
base_url = "http://localhost:11434/v1"
api_key = "ollama"

[providers.openrouter]
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"

[marp]
pdf = false
"#;
    fs::write(&path, old).unwrap();
    migrate_global_config(&path).unwrap();
    let migrated: GlobalConfig = read_toml(&path).unwrap();
    assert!(migrated.connectors.contains_key("ollama"));
    assert!(migrated.connectors.contains_key("openrouter"));
    assert_eq!(
        migrated
            .defaults
            .0
            .get(&Capability::Text)
            .map(String::as_str),
        Some("local-text")
    );
    let profile = migrated.models.get("local-text").unwrap();
    assert_eq!(profile.connector, "ollama");
    assert_eq!(profile.model, "llama3.2");
    assert_eq!(
        profile
            .options
            .get("temperature")
            .and_then(toml::Value::as_float),
        Some(0.5)
    );
    let rendered = fs::read_to_string(&path).unwrap();
    assert!(!rendered.contains("[inference]"));
    assert!(!rendered.contains("[providers."));
    assert!(PathBuf::from(format!("{}.bak", path.display())).exists());
}
