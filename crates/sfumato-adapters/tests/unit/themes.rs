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

#[test]
fn imports_and_exports_design_md_with_renderer_adapters() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemThemeRepository::new(temp.path().join("themes"));
    let design_path = temp.path().join("DESIGN.md");
    fs::write(
        &design_path,
        r##"---
version: alpha
name: Gruvbox Study
description: Warm high-contrast study materials.
colors:
  primary: "#d79921"
  background: "#282828"
  surface: "#3c3836"
  text: "#ebdbb2"
typography:
  h1:
    fontFamily: Atkinson Hyperlegible
  body-md:
    fontFamily: Inter
---

## Overview

Warm and focused.

## Colors

Use semantic contrast.
"##,
    )
    .unwrap();

    let imported = repository
        .import_design(design_path, Some("gruvbox-study"))
        .unwrap();
    assert_eq!(imported.manifest.tokens.colors["background"], "#282828");
    assert_eq!(
        imported.manifest.tokens.fonts["heading"],
        "Atkinson Hyperlegible"
    );
    assert!(imported.root.join("DESIGN.md").is_file());
    assert!(
        fs::read_to_string(imported.marp_css_path())
            .unwrap()
            .contains("#282828")
    );

    let export_path = temp.path().join("exported/DESIGN.md");
    repository
        .export_design("gruvbox-study", export_path.clone())
        .unwrap();
    let exported = fs::read_to_string(export_path).unwrap();
    assert!(exported.starts_with("---\nversion: alpha"));
    assert!(exported.contains("## Colors"));
    assert_eq!(
        parse_design_document(&exported).unwrap().name,
        "gruvbox-study"
    );
}

#[test]
fn rejects_invalid_design_tokens_and_duplicate_sections() {
    let invalid_color =
        "---\nversion: alpha\nname: Bad\ncolors:\n  primary: red\n---\n\n## Colors\n";
    assert!(parse_design_document(invalid_color).is_err());

    let duplicate = "---\nversion: alpha\nname: Bad\ncolors:\n  primary: '#ffffff'\n---\n\n## Colors\n\n## Colors\n";
    assert!(parse_design_document(duplicate).is_err());
}
