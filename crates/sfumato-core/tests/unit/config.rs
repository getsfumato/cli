use super::*;

fn effective_config() -> EffectiveConfig {
    let global = GlobalConfig::default_config();
    EffectiveConfig {
        user: global.user,
        project_name: "university".to_string(),
        project_root: PathBuf::from("/tmp/university"),
        publish_dir: None,
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
fn rejects_legacy_project_config_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let project_path = temp.path().join("project.toml");
    let legacy = "schema_version = 2\nname = \"demo\"\ntheme = \"gruvbox\"\n";
    fs::write(&project_path, legacy).unwrap();

    let error = load_project_config(&project_path, "ignored").unwrap_err();

    assert!(!format!("{error:#}").is_empty());
    assert_eq!(fs::read_to_string(project_path).unwrap(), legacy);
}

#[test]
fn artifact_root_is_centralized_outside_the_project_source() {
    let root = project_artifact_root("University").unwrap();

    assert!(root.ends_with(".sfumato/Projects/University"));
    assert!(validate_project_name("../University").is_err());
}

#[test]
fn publish_root_resolves_relative_to_the_project_without_changing_artifacts() {
    let mut config = effective_config();
    config.project_root = PathBuf::from("/tmp/source-vault");
    config.publish_dir = Some(PathBuf::from("Published Slides"));

    assert_eq!(
        config.publish_root().unwrap(),
        Some(PathBuf::from("/tmp/source-vault/Published Slides"))
    );
    assert!(
        config
            .artifact_root()
            .unwrap()
            .ends_with(".sfumato/Projects/university")
    );
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
fn rejects_future_global_config_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let future = "schema_version = 99\n[user]\nlearning_style = []\n";
    fs::write(&path, future).unwrap();

    let error = read_toml::<GlobalConfig>(&path).unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("version") && message.contains("99"));
    assert_eq!(fs::read_to_string(path).unwrap(), future);
}
