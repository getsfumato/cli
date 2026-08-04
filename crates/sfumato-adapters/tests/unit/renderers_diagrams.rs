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

#[test]
fn the_browser_sandbox_is_on_by_default() {
    // `--no-sandbox` was the entire args array, unconditional and unexplained, for
    // input that is Mermaid source written by a model and never reviewed. Rendering
    // was measured to work with the sandbox enabled, so nothing is traded away.
    assert!(sandbox_args(None).is_empty());
}

#[test]
fn the_sandbox_can_be_opted_out_for_an_environment_that_cannot_run_it() {
    // A container running as root is the case that needs this; naming it by
    // environment keeps it from being everyone's default.
    for value in ["1", "true", "TRUE", " 1 "] {
        assert_eq!(
            sandbox_args(Some(value)),
            vec!["--no-sandbox"],
            "value {value:?}"
        );
    }
}

#[test]
fn an_unrecognised_opt_out_value_leaves_the_sandbox_on() {
    // Anything that is not a clear yes keeps the protection: a stray value must
    // not quietly disable a security control.
    for value in ["0", "false", "", "yes", "no", "on"] {
        assert!(sandbox_args(Some(value)).is_empty(), "value {value:?}");
    }
}
