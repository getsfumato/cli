use super::*;

#[test]
fn mermaid_cli_args_render_svg_from_source_file() {
    let args = mermaid_cli_args(
        Path::new("diagram.mmd"),
        Path::new("diagram.svg"),
        None,
        None,
    );
    assert_eq!(
        args,
        vec![
            OsString::from("-i"),
            OsString::from("diagram.mmd"),
            OsString::from("-o"),
            OsString::from("diagram.svg"),
            OsString::from("--backgroundColor"),
            OsString::from("transparent"),
        ]
    );
}

#[test]
fn mermaid_cli_args_include_optional_configs() {
    let args = mermaid_cli_args(
        Path::new("diagram.mmd"),
        Path::new("diagram.svg"),
        Some(Path::new("puppeteer.json")),
        Some(Path::new("mermaid.json")),
    );
    assert!(
        args.windows(2)
            .any(|window| { window == [OsString::from("-p"), OsString::from("puppeteer.json")] })
    );
    assert!(
        args.windows(2)
            .any(|window| { window == [OsString::from("-c"), OsString::from("mermaid.json")] })
    );
}

#[test]
fn mermaid_theme_config_uses_base_theme_for_custom_variables() {
    let config = MermaidThemeConfig::new(std::collections::BTreeMap::from([(
        "primaryColor".to_string(),
        "#fbf1c7".to_string(),
    )]));
    let rendered = serde_json::to_string(&config).unwrap();
    assert!(rendered.contains("\"theme\":\"base\""));
    assert!(rendered.contains("\"themeVariables\""));
}

#[test]
fn validate_svg_rejects_non_svg_output() {
    assert!(
        validate_svg("not svg")
            .unwrap_err()
            .to_string()
            .contains("SVG document")
    );
}
