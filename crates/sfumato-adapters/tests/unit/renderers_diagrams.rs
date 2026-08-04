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

#[test]
fn the_configured_browser_path_reaches_the_diagram_renderer() {
    // `marp.browser_path` reached the slide and page renderers but had nowhere to go
    // for diagrams, because the port had no parameter for it — so a user whose
    // browser is outside /Applications could render slides but not diagrams.
    let temporary = tempfile::tempdir().unwrap();
    let browser = temporary.path().join("my-chrome");
    std::fs::write(&browser, "#!/bin/sh\n").unwrap();
    let output = temporary.path().join("diagram.svg");

    let config = write_puppeteer_config(&output, Some(&browser))
        .expect("a configured browser is accepted")
        .expect("a config is written");

    let written = std::fs::read_to_string(&config).unwrap();
    assert!(
        written.contains(browser.to_str().unwrap()),
        "the configured path is not in the config: {written}"
    );
}

#[test]
fn a_configured_browser_that_does_not_exist_is_reported() {
    // Silently falling back to a scan would render with a browser the user did not
    // choose, which is the failure `resolved_browser_path` already reports for the
    // other renderers.
    let temporary = tempfile::tempdir().unwrap();
    let missing = temporary.path().join("nope");

    let error = write_puppeteer_config(&temporary.path().join("d.svg"), Some(&missing))
        .expect_err("a missing configured browser is refused");

    assert!(format!("{error:#}").contains("does not exist"), "{error:#}");
}
