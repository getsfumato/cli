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
        plugins: Vec::new(),
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
fn publish_root_resolves_relative_to_the_project_without_changing_artifacts() {
    let mut config = effective_config();
    config.project_root = PathBuf::from("/tmp/source-vault");
    config.publish_dir = Some(PathBuf::from("Published Slides"));

    assert_eq!(
        config.publish_root().unwrap(),
        Some(PathBuf::from("/tmp/source-vault/Published Slides"))
    );
    assert!(validate_project_name("../University").is_err());
}

#[test]
fn new_global_config_is_valid() {
    let config = GlobalConfig::default_config();
    config.validate().unwrap();
    assert!(config.models.contains_key("local-text"));
    assert_eq!(
        config.defaults.0.get(&Capability::Text).map(String::as_str),
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
