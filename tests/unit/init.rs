use super::*;

#[test]
fn default_user_config_contains_named_text_profile() {
    let config = GlobalConfig::default_config();
    let profile = config.models.get("local-text").unwrap();
    assert_eq!(profile.connector, "ollama");
    assert!(profile.capabilities.contains(&Capability::Text));
    assert_eq!(
        config.defaults.0.get(&Capability::Text).map(String::as_str),
        Some("local-text")
    );
}
