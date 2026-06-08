use super::*;

#[test]
fn renders_interactive_user_config_answers() {
    let rendered = render_user_config_template(&UserInitAnswers {
        name: "Alex".to_string(),
        learning_style: vec!["visual".to_string(), "practice".to_string()],
        theme: "sfumato-default".to_string(),
        provider: "openrouter".to_string(),
        model: "openai/gpt-4o-mini".to_string(),
        temperature: 0.3,
        max_tokens: 3000,
        openrouter_api_key_env: "OPENROUTER_API_KEY".to_string(),
        marp_theme: "default".to_string(),
        marp_pdf: true,
    });

    let parsed = toml::from_str::<crate::config::PartialConfig>(&rendered).unwrap();

    let user = parsed.user.unwrap();
    assert_eq!(user.name.as_deref(), Some("Alex"));
    assert_eq!(user.learning_style, vec!["visual", "practice"]);

    let inference = parsed.inference.unwrap();
    assert_eq!(inference.provider, "openrouter");
    assert_eq!(inference.model, "openai/gpt-4o-mini");

    let marp = parsed.marp.unwrap();
    assert!(marp.pdf);
}
