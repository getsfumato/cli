use super::*;

#[test]
fn built_in_registry_contains_only_valid_cdn_recipes() {
    let registry = parse_registry(BUILTIN_REGISTRY).unwrap();
    assert_eq!(registry.schema_version, 2);
    assert_eq!(registry.plugins.len(), 8);
    assert!(registry.plugins.iter().any(|plugin| plugin.id == "shadcn"));

    for plugin in registry.plugins {
        for release in plugin.releases {
            assert!(!release.licenses.is_empty());
            match release.install {
                PagePluginInstallRecipe::ClassicGlobal { runtime, .. } => {
                    assert!(runtime.url.contains("cdn.jsdelivr.net"));
                }
                PagePluginInstallRecipe::EsmNamespace { entry, .. } => {
                    assert!(entry.url.contains("cdn.jsdelivr.net") || entry.url.contains("esm.sh"));
                }
                PagePluginInstallRecipe::TailwindSource { runtime, sources } => {
                    assert!(runtime.url.contains("cdn.jsdelivr.net"));
                    assert!(
                        sources
                            .iter()
                            .all(|source| source.url.contains("ui.shadcn.com"))
                    );
                }
            }
        }
    }
}

#[test]
fn rejects_unsafe_browser_global_expressions() {
    assert!(validate_global_expression("window.SfumatoPlugins.motion").is_ok());
    assert!(validate_global_expression("window.x;alert(1)").is_err());
    assert!(validate_global_expression("globalThis.plugin").is_err());
}

fn package(id: &str, version: &str, dependencies: Vec<String>) -> DownloadedPagePluginPackage {
    let runtime = format!("window.SfumatoPlugins['{id}'] = {{}};");
    DownloadedPagePluginPackage {
        schema_version: 1,
        id: id.into(),
        name: id.into(),
        version: version.into(),
        api_global: format!("window.SfumatoPlugins.{id}"),
        category: if id == "react" {
            sfumato_core::page_plugins::PagePluginCategory::Runtime
        } else {
            sfumato_core::page_plugins::PagePluginCategory::Ui
        },
        dependencies,
        runtime_hash: format!("{:x}", Sha256::digest(runtime.as_bytes())),
        runtime_javascript: runtime,
        stylesheet: format!(".{id} {{ display: block; }}"),
        guidance: format!("Use {id}."),
        license: "LICENSE".into(),
        license_text: "MIT".into(),
    }
}

#[test]
fn filesystem_catalog_installs_and_lists_selected_versions() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = FilesystemPagePluginCatalog::new(directory.path().to_path_buf());
    catalog
        .install(package("shadcn", "1.0.0", Vec::new()))
        .unwrap();
    catalog
        .install(package("shadcn", "1.1.0", Vec::new()))
        .unwrap();

    let plugins = catalog.list().unwrap().entries;
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "shadcn");
    assert_eq!(plugins[0].version, "1.1.0");
    assert!(
        catalog
            .load("shadcn")
            .unwrap()
            .stylesheet
            .contains(".shadcn")
    );
}

#[test]
fn resolves_installed_dependencies_before_requested_plugin() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = FilesystemPagePluginCatalog::new(directory.path().to_path_buf());
    catalog.install(package("react", "1", Vec::new())).unwrap();
    catalog
        .install(package("shadcn", "1", vec!["react".into()]))
        .unwrap();

    let ids = catalog
        .resolve(&["shadcn".into()])
        .unwrap()
        .into_iter()
        .map(|plugin| plugin.summary.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["react", "shadcn"]);
}

#[test]
fn rejects_unknown_installed_plugins() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = FilesystemPagePluginCatalog::new(directory.path().to_path_buf());
    let error = catalog.load("unknown").unwrap_err();
    assert!(error.to_string().contains("not installed"));
}

#[test]
fn rejects_resolving_two_ui_libraries_together() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = FilesystemPagePluginCatalog::new(directory.path().to_path_buf());
    catalog.install(package("shadcn", "1", Vec::new())).unwrap();
    catalog
        .install(package("materialui", "1", Vec::new()))
        .unwrap();

    let error = catalog
        .resolve(&["shadcn".into(), "materialui".into()])
        .unwrap_err();

    assert!(error.to_string().contains("only one UI library"));
}
