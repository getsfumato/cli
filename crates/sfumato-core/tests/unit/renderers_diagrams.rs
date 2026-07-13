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
            std::ffi::OsString::from("-i"),
            std::ffi::OsString::from("diagram.mmd"),
            std::ffi::OsString::from("-o"),
            std::ffi::OsString::from("diagram.svg"),
            std::ffi::OsString::from("--backgroundColor"),
            std::ffi::OsString::from("transparent"),
        ]
    );
}

#[test]
fn mermaid_cli_args_request_transparent_background() {
    let args = mermaid_cli_args(
        Path::new("diagram.mmd"),
        Path::new("diagram.svg"),
        None,
        None,
    );

    assert!(args.windows(2).any(|window| {
        window
            == [
                std::ffi::OsString::from("--backgroundColor"),
                std::ffi::OsString::from("transparent"),
            ]
    }));
}

#[test]
fn mermaid_cli_args_include_puppeteer_config_when_available() {
    let args = mermaid_cli_args(
        Path::new("diagram.mmd"),
        Path::new("diagram.svg"),
        Some(Path::new("puppeteer.json")),
        None,
    );

    assert!(args.windows(2).any(|window| {
        window
            == [
                std::ffi::OsString::from("-p"),
                std::ffi::OsString::from("puppeteer.json"),
            ]
    }));
}

#[test]
fn mermaid_cli_args_include_mermaid_config_when_available() {
    let args = mermaid_cli_args(
        Path::new("diagram.mmd"),
        Path::new("diagram.svg"),
        None,
        Some(Path::new("mermaid.json")),
    );

    assert!(args.windows(2).any(|window| {
        window
            == [
                std::ffi::OsString::from("-c"),
                std::ffi::OsString::from("mermaid.json"),
            ]
    }));
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
    let error = validate_svg("not svg").unwrap_err();

    assert!(error.to_string().contains("SVG document"));
}
