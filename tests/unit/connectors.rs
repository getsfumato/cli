use super::*;

#[test]
fn default_config_has_openai_compatible_connector_presets() {
    let service = ConnectorService {
        config: GlobalConfig::default_config(),
    };

    assert_eq!(
        service.config.connectors.get("ollama").unwrap().base_url,
        "http://localhost:11434/v1"
    );
    assert_eq!(
        service
            .config
            .connectors
            .get("openrouter")
            .unwrap()
            .base_url,
        "https://openrouter.ai/api/v1"
    );
}
