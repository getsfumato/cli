use super::*;
#[cfg(feature = "real-renderers")]
use sfumato_core::renderers::SlideRenderer;

#[test]
fn marp_command_includes_custom_theme_css() {
    let args = command_args(
        Path::new("deck.md"),
        Path::new("themes/gruvbox.css"),
        Path::new("deck.pdf"),
        None,
    )
    .unwrap();
    assert!(args.windows(2).any(|window| {
        window
            == [
                OsString::from("--theme"),
                OsString::from("themes/gruvbox.css"),
            ]
    }));
    assert!(args.contains(&OsString::from("--allow-local-files")));
}

#[test]
fn marp_command_prefers_configured_browser_path() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let args = command_args(
        Path::new("deck.md"),
        Path::new("theme.css"),
        Path::new("deck.pdf"),
        Some(temp.path()),
    )
    .unwrap();
    assert!(args.windows(2).any(|window| {
        window
            == [
                OsString::from("--browser-path"),
                temp.path().as_os_str().to_owned(),
            ]
    }));
}

#[test]
fn marp_command_rejects_missing_configured_browser_path() {
    let error = command_args(
        Path::new("deck.md"),
        Path::new("theme.css"),
        Path::new("deck.pdf"),
        Some(Path::new("/missing/sfumato-browser")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("Configured Marp browser path"));
}

#[test]
fn parses_percent_encoded_layout_report() {
    let html = r#"<html data-sfumato-layout="%5B%7B%22slide%22%3A2%2C%22title%22%3A%22Dense%22%2C%22vertical_overflow_px%22%3A48%2C%22horizontal_overflow_px%22%3A0%7D%5D">"#;
    let issues = parse_layout_report(html).unwrap();
    assert_eq!(issues[0].slide, 2);
    assert_eq!(issues[0].vertical_overflow_px, 48);
}

#[test]
fn injects_layout_inspection_script() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), "<html><body><section></section></body></html>").unwrap();
    inject_layout_inspector(temp.path()).unwrap();
    let rendered = std::fs::read_to_string(temp.path()).unwrap();
    assert!(rendered.contains("data-marpit-svg"));
    assert!(rendered.contains("sfumatoLayout"));
}

#[tokio::test]
#[cfg(feature = "real-renderers")]
async fn detects_real_overflow_when_dependencies_are_available() {
    let Some(browser) = detected_browser_path() else {
        return;
    };
    if std::process::Command::new("marp")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let markdown = temp.path().join("deck.md");
    let theme = temp.path().join("theme.css");
    let html = temp.path().join("deck.html");
    std::fs::write(
        &theme,
        "/* @theme overflow-test */\n@import 'default';\nsection { font-size: 40px; }",
    )
    .unwrap();
    let dense = (0..40)
        .map(|index| format!("- Overflow line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&markdown, format!("---\nmarp: true\ntheme: overflow-test\n---\n\n# Dense\n\n{dense}\n\n---\n\n# Short\n\nFits.")).unwrap();
    let issues = MarpCliRenderer
        .inspect_layout(
            &markdown,
            &theme,
            &html,
            Some(&browser),
            &sfumato_core::operation::OperationContext::detached(),
        )
        .await
        .unwrap();
    assert_eq!(issues.first().map(|issue| issue.slide), Some(1));
}
