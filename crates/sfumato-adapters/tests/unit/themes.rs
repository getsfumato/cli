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

#[test]
fn installs_every_file_the_default_theme_manifest_declares() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemThemeRepository::new(temp.path().join("themes"));

    let package = repository.install_default().unwrap();

    // `document/print.css` was declared by the manifest but never written, so
    // `generate document` failed at the render stage on every fresh install.
    let document_css = package
        .document_css_path()
        .expect("declares a document adapter");
    assert!(document_css.is_file(), "missing {}", document_css.display());
    assert!(package.marp_css_path().is_file());
}

#[test]
fn repairs_a_default_theme_installed_without_its_document_stylesheet() {
    let temp = tempfile::tempdir().unwrap();
    let themes = temp.path().join("themes");
    let repository = FilesystemThemeRepository::new(themes.clone());
    repository.install_default().unwrap();

    // Reproduce what an earlier release left on disk.
    let document_css = themes.join("sfumato-default/document/print.css");
    fs::remove_file(&document_css).unwrap();
    assert!(!document_css.is_file());

    repository.install_default().unwrap();

    assert!(document_css.is_file(), "install did not restore the file");
}

#[test]
fn keeps_user_edits_to_an_already_installed_default_theme() {
    let temp = tempfile::tempdir().unwrap();
    let themes = temp.path().join("themes");
    let repository = FilesystemThemeRepository::new(themes.clone());
    repository.install_default().unwrap();

    let style = themes.join("sfumato-default/html/style.css");
    fs::write(&style, "/* hand tuned */").unwrap();
    repository.install_default().unwrap();

    // Repair restores what is missing; it must not overwrite what is present.
    assert_eq!(fs::read_to_string(&style).unwrap(), "/* hand tuned */");
}

#[test]
fn repairs_a_custom_theme_copied_from_a_broken_default() {
    let temp = tempfile::tempdir().unwrap();
    let themes = temp.path().join("themes");
    let repository = FilesystemThemeRepository::new(themes.clone());
    repository.install_default().unwrap();
    repository.create("gruvbox").unwrap();

    // What `theme create` produced while the bundled list was incomplete.
    let document_css = themes.join("gruvbox/document/print.css");
    fs::remove_file(&document_css).unwrap();

    // Loading must heal it, not refuse: refusing would break every command for
    // a theme the user already had, instead of only `generate document`.
    let package = repository.load("gruvbox").unwrap();

    assert!(document_css.is_file());
    assert_eq!(package.document_css_path().unwrap(), document_css);
}

#[test]
fn accepts_a_theme_that_declares_no_document_adapter() {
    let temp = tempfile::tempdir().unwrap();
    let themes = temp.path().join("themes");
    let repository = FilesystemThemeRepository::new(themes.clone());
    repository.install_default().unwrap();
    repository.create("marp-only").unwrap();

    // Themes predating documents omit the section and fall back to a bundled
    // stylesheet; validation must leave them alone.
    let manifest_path = themes.join("marp-only/theme.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let trimmed = manifest
        .lines()
        .take_while(|line| !line.starts_with("[adapters.document]"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&manifest_path, trimmed).unwrap();
    fs::remove_dir_all(themes.join("marp-only/document")).unwrap();

    let package = repository.load("marp-only").unwrap();
    assert!(package.document_css_path().is_none());
}

#[test]
fn never_repairs_a_document_path_that_escapes_the_theme_package() {
    let temp = tempfile::tempdir().unwrap();
    let themes = temp.path().join("themes");
    let repository = FilesystemThemeRepository::new(themes.clone());
    repository.install_default().unwrap();
    repository.create("escaping").unwrap();

    let manifest_path = themes.join("escaping/theme.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap().replace(
        "css = \"document/print.css\"",
        "css = \"../../escaped.css\"",
    );
    fs::write(&manifest_path, manifest).unwrap();
    fs::remove_file(themes.join("escaping/document/print.css")).unwrap();

    // Repair must defer to validation here rather than writing outside the
    // package it is repairing.
    let error = repository.load("escaping").unwrap_err().to_string();
    assert!(
        error.contains("must stay inside the theme package"),
        "{error}"
    );
    assert!(!temp.path().join("escaped.css").exists());
}

#[test]
fn rejects_a_theme_whose_colour_token_is_not_hex() {
    let temp = tempfile::tempdir().unwrap();
    let themes = temp.path().join("themes");
    let repository = FilesystemThemeRepository::new(themes.clone());
    repository.install_default().unwrap();

    // `tokens.colors` had no validation at all, and the chart tool parses these
    // values by byte-slicing: an accented typo panicked it.
    let path = themes.join("sfumato-default/theme.toml");
    let manifest = fs::read_to_string(&path)
        .unwrap()
        .replace("primary = \"#315c8c\"", "primary = \"#abcñd\"");
    fs::write(&path, manifest).unwrap();

    let error = repository.load(DEFAULT_THEME).unwrap_err().to_string();
    assert!(error.contains("Theme colour 'primary'"), "{error}");
}

#[test]
fn rejects_a_theme_colour_that_is_a_css_name_rather_than_hex() {
    let temp = tempfile::tempdir().unwrap();
    let themes = temp.path().join("themes");
    let repository = FilesystemThemeRepository::new(themes.clone());
    repository.install_default().unwrap();

    // Silent degradation, not a panic: an unparseable colour was read as light,
    // so a dark theme quietly produced a light chart.
    let path = themes.join("sfumato-default/theme.toml");
    let manifest = fs::read_to_string(&path)
        .unwrap()
        .replace("background = \"#f7f7f5\"", "background = \"red\"");
    fs::write(&path, manifest).unwrap();

    assert!(repository.load(DEFAULT_THEME).is_err());
}

#[test]
fn the_bundled_theme_colours_all_validate() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemThemeRepository::new(temp.path().join("themes"));

    // The new validation must not reject what the project itself ships.
    let package = repository.install_default().unwrap();
    assert!(!package.manifest.tokens.colors.is_empty());
}
