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
