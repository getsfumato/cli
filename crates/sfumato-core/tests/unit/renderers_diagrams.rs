use super::*;

#[test]
fn mermaid_cli_args_render_svg_from_source_file() {
    let args = mermaid_cli_args(Path::new("diagram.mmd"), Path::new("diagram.svg"), None);

    assert_eq!(
        args,
        vec![
            std::ffi::OsString::from("-i"),
            std::ffi::OsString::from("diagram.mmd"),
            std::ffi::OsString::from("-o"),
            std::ffi::OsString::from("diagram.svg"),
        ]
    );
}

#[test]
fn mermaid_cli_args_include_puppeteer_config_when_available() {
    let args = mermaid_cli_args(
        Path::new("diagram.mmd"),
        Path::new("diagram.svg"),
        Some(Path::new("puppeteer.json")),
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
fn validate_svg_rejects_non_svg_output() {
    let error = validate_svg("not svg").unwrap_err();

    assert!(error.to_string().contains("SVG document"));
}
