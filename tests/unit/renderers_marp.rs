use super::*;

#[test]
fn marp_command_includes_custom_theme_css() {
    let args = command_args(
        Path::new("deck.md"),
        Path::new("themes/gruvbox.css"),
        Path::new("deck.pdf"),
    );
    assert_eq!(
        args,
        vec!["--theme", "themes/gruvbox.css", "deck.md", "-o", "deck.pdf"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
    );
}
