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
    fs::write(&global_path, toml::to_string_pretty(&global_value).unwrap()).unwrap();
    migrate_global_config(&global_path).unwrap();
    let migrated_global: GlobalConfig = read_toml(&global_path).unwrap();
    assert_eq!(migrated_global.schema_version, CONFIG_SCHEMA_VERSION);
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
fn old_single_inference_config_is_rejected() {
    let old = r#"
[user]
learning_style = ["visual"]
theme = "default"

[inference]
provider = "ollama"
model = "llama3.2"
temperature = 0.4
max_tokens = 4000
"#;
    assert!(toml::from_str::<GlobalConfig>(old).is_err());
}
