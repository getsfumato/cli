use super::*;

#[test]
fn cli_overrides_take_precedence() {
    let mut config = SfumatoConfig::default_for_cwd(PathBuf::from("/tmp/vault"));
    config.apply_overrides(ConfigOverrides {
        provider: Some(ProviderArg::Openrouter),
        model: Some("openai/gpt-4o-mini".to_string()),
        output_dir: Some(PathBuf::from("Generated")),
        pdf: true,
        config_path: None,
    });

    assert_eq!(config.inference.provider, "openrouter");
    assert_eq!(config.inference.model, "openai/gpt-4o-mini");
    assert_eq!(config.project.output_dir, PathBuf::from("Generated"));
    assert!(config.marp.pdf);
}
