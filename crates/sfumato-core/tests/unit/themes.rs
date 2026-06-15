use super::*;
use crate::config::ProjectRegistry;

#[test]
fn creates_lists_and_resolves_theme_packages() {
    let temp = tempfile::tempdir().unwrap();
    let service = ThemeService::load_from(temp.path().join("themes"));

    service.install_default().unwrap();
    service.create("gruvbox").unwrap();

    assert_eq!(
        service.names().unwrap(),
        vec!["gruvbox".to_string(), "sfumato-default".to_string()]
    );
    let package = service.resolve("gruvbox").unwrap();
    assert!(package.marp_css_path().is_file());
    assert!(
        fs::read_to_string(package.marp_css_path())
            .unwrap()
            .contains("/* @theme gruvbox */")
    );
    assert!(service.create("gruvbox").is_err());
}

#[test]
fn rejects_invalid_names_and_unsafe_adapter_paths() {
    let temp = tempfile::tempdir().unwrap();
    let service = ThemeService::load_from(temp.path().join("themes"));
    assert!(service.create("../bad").is_err());

    service.install_default().unwrap();
    let path = temp.path().join("themes/sfumato-default/theme.toml");
    let mut manifest: ThemeManifest = crate::config::read_toml(&path).unwrap();
    manifest.adapters.marp_css = PathBuf::from("../outside.css");
    crate::config::write_toml(&path, &manifest).unwrap();
    assert!(service.resolve(DEFAULT_THEME).is_err());
}

#[test]
fn rejects_html_shell_without_exactly_one_content_slot() {
    let temp = tempfile::tempdir().unwrap();
    let service = ThemeService::load_from(temp.path().join("themes"));
    service.install_default().unwrap();
    fs::write(
        temp.path().join("themes/sfumato-default/html/page.html"),
        "<html></html>",
    )
    .unwrap();
    assert!(service.resolve(DEFAULT_THEME).is_err());
}

#[test]
fn assigns_theme_to_active_or_explicit_project() {
    let temp = tempfile::tempdir().unwrap();
    let service = ThemeService::load_from(temp.path().join("themes"));
    service.install_default().unwrap();
    service.create("gruvbox").unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    for (root, name) in [(&first, "first"), (&second, "second")] {
        crate::config::write_toml(
            &crate::config::project_config_path(root),
            &crate::config::ProjectConfig {
                schema_version: crate::config::CONFIG_SCHEMA_VERSION,
                name: name.to_string(),
                theme: DEFAULT_THEME.to_string(),
                output_dir: PathBuf::from("Resources/Sfumato"),
                model_defaults: Default::default(),
                marp: None,
            },
        )
        .unwrap();
    }
    let registry = ProjectRegistry {
        schema_version: crate::config::CONFIG_SCHEMA_VERSION,
        active: Some("first".to_string()),
        projects: BTreeMap::from([
            (
                "first".to_string(),
                crate::config::RegisteredProject {
                    path: first.clone(),
                },
            ),
            (
                "second".to_string(),
                crate::config::RegisteredProject {
                    path: second.clone(),
                },
            ),
        ]),
    };

    service
        .use_for_project_in_registry("gruvbox", None, &registry)
        .unwrap();
    service
        .use_for_project_in_registry("gruvbox", Some("second"), &registry)
        .unwrap();
    for root in [first, second] {
        let project = crate::config::load_project_config(
            &crate::config::project_config_path(&root),
            DEFAULT_THEME,
        )
        .unwrap();
        assert_eq!(project.theme, "gruvbox");
    }
}
