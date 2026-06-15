use super::*;

#[test]
fn default_config_has_openai_compatible_connector_presets() {
    let temp = tempfile::tempdir().unwrap();
    let repository =
        crate::repositories::FilesystemGlobalConfigRepository::new(temp.path().join("config.toml"));
    crate::repositories::GlobalConfigRepository::save(&repository, &GlobalConfig::default_config())
        .unwrap();
    let service = ConnectorService::new(Box::new(repository)).unwrap();

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
