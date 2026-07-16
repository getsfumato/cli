use super::*;

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
    assert!(args.contains(&OsString::from("deck.md")));
    assert!(args.contains(&OsString::from("--allow-local-files")));
    assert!(
        args.windows(2)
            .any(|window| { window == [OsString::from("-o"), OsString::from("deck.pdf")] })
    );
    if detected_browser_path().is_some() {
        assert!(args.contains(&OsString::from("--browser-path")));
    }
}

#[test]
fn marp_command_prefers_configured_browser_path() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let browser_path = temp.path();
    let args = command_args(
        Path::new("deck.md"),
        Path::new("themes/gruvbox.css"),
        Path::new("deck.pdf"),
        Some(browser_path),
    )
    .unwrap();

    assert!(args.windows(2).any(|window| {
        window
            == [
                OsString::from("--browser-path"),
                browser_path.as_os_str().to_owned(),
            ]
    }));
}

#[test]
fn marp_command_rejects_missing_configured_browser_path() {
    let error = command_args(
        Path::new("deck.md"),
        Path::new("themes/gruvbox.css"),
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

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].slide, 2);
    assert_eq!(issues[0].vertical_overflow_px, 48);
}

#[test]
fn injects_layout_inspection_without_touching_source_markdown() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), "<html><body><section></section></body></html>").unwrap();

    inject_layout_inspector(temp.path()).unwrap();
    let rendered = std::fs::read_to_string(temp.path()).unwrap();

    assert!(rendered.contains("data-sfumato-svg") || rendered.contains("data-marpit-svg"));
    assert!(rendered.contains("sfumatoLayout"));
}

#[tokio::test]
#[cfg(feature = "real-renderers")]
async fn detects_real_overflow_when_marp_and_browser_are_available() {
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
    let dense_content = (0..40)
        .map(|index| format!("- This is deliberately overflowing content line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        &markdown,
        format!(
            "---\nmarp: true\ntheme: overflow-test\n---\n\n# Dense slide\n\n{dense_content}\n\n---\n\n# Short slide\n\nThis slide fits."
        ),
    )
    .unwrap();

    let issues = inspect_layout(&markdown, &theme, &html, Some(&browser))
        .await
        .unwrap();

    assert_eq!(issues.first().map(|issue| issue.slide), Some(1));
    assert!(issues[0].vertical_overflow_px > 0);
    assert!(issues.iter().all(|issue| issue.slide == 1));
}
