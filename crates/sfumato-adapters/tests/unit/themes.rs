use super::*;
use crate::repositories::FilesystemProjectRepository;
use sfumato_core::{repositories::ProjectRepository, themes::ThemeService};
use std::sync::Arc;

#[test]
fn creates_lists_and_resolves_theme_packages() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemThemeRepository::new(temp.path().join("themes"));

    repository.install_default().unwrap();
    repository.create("gruvbox").unwrap();

    let names = repository
        .list()
        .unwrap()
        .into_iter()
        .map(|theme| theme.name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["gruvbox", "sfumato-default"]);
    let package = repository.load("gruvbox").unwrap();
    assert!(package.marp_css_path().is_file());
    assert!(
        fs::read_to_string(package.marp_css_path())
            .unwrap()
            .contains("/* @theme gruvbox */")
    );
    assert!(repository.create("gruvbox").is_err());
}

#[test]
fn rejects_invalid_names_and_unsafe_adapter_paths() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemThemeRepository::new(temp.path().join("themes"));
    assert!(repository.create("../bad").is_err());

    repository.install_default().unwrap();
    let path = temp.path().join("themes/sfumato-default/theme.toml");
    let mut manifest: ThemeManifest = read_toml(&path).unwrap();
    manifest.adapters.marp_css = PathBuf::from("../outside.css");
    write_toml(&path, &manifest).unwrap();
    assert!(repository.load(DEFAULT_THEME).is_err());
}

#[test]
fn rejects_html_shell_without_exactly_one_content_slot() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemThemeRepository::new(temp.path().join("themes"));
    repository.install_default().unwrap();
    fs::write(
        temp.path().join("themes/sfumato-default/html/page.html"),
        "<html></html>",
    )
    .unwrap();
    assert!(repository.load(DEFAULT_THEME).is_err());
}

#[test]
fn assigns_theme_to_active_or_explicit_project() {
    let temp = tempfile::tempdir().unwrap();
    let themes = Arc::new(FilesystemThemeRepository::new(temp.path().join("themes")));
    let projects = Arc::new(FilesystemProjectRepository::new(
        temp.path().join("projects.toml"),
    ));
    let service = ThemeService::new(themes.clone(), projects.clone());
    service.install_default().unwrap();
    service.create("gruvbox").unwrap();

    projects
        .register("first".to_string(), temp.path().join("first"), true)
        .unwrap();
    projects
        .register("second".to_string(), temp.path().join("second"), false)
        .unwrap();

    service.use_for_project("gruvbox", None).unwrap();
    service.use_for_project("gruvbox", Some("second")).unwrap();
    for name in ["first", "second"] {
        let project = projects.load(Some(name)).unwrap();
        assert_eq!(project.theme, "gruvbox");
    }
}
