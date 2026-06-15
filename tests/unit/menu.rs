use super::*;

#[test]
fn menu_lists_every_top_level_command_family() {
    assert_eq!(MAIN_MENU.len(), 8);
    assert!(MAIN_MENU.contains(&"Generate resources"));
    assert!(MAIN_MENU.contains(&"Projects"));
    assert!(MAIN_MENU.contains(&"Themes"));
    assert!(MAIN_MENU.contains(&"Connectors"));
    assert!(MAIN_MENU.contains(&"Models"));
    assert!(MAIN_MENU.contains(&"Configuration"));
    assert!(MAIN_MENU.contains(&"Setup"));
}

#[test]
fn parses_comma_separated_inputs_and_overrides() {
    assert_eq!(
        parse_paths("notes, course material/week-1,"),
        vec![
            PathBuf::from("notes"),
            PathBuf::from("course material/week-1")
        ]
    );
    assert_eq!(
        parse_list("text=local-text, image=cloud-image"),
        vec!["text=local-text", "image=cloud-image"]
    );
}
