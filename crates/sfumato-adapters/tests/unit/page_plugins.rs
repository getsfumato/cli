use super::*;

#[test]
fn bundled_catalog_lists_plugins_deterministically() {
    let catalog = BundledPagePluginCatalog;
    let ids = catalog
        .list()
        .unwrap()
        .into_iter()
        .map(|plugin| plugin.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "lottie",
            "materialui",
            "motion",
            "react",
            "react-dom",
            "theatre",
            "threejs"
        ]
    );
}

#[test]
fn resolves_react_dependencies_before_material_ui() {
    let catalog = BundledPagePluginCatalog;
    let plugins = catalog.resolve(&["materialui".into()]).unwrap();
    let ids = plugins
        .into_iter()
        .map(|plugin| plugin.summary.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["react", "react-dom", "materialui"]);
}

#[test]
fn bundled_catalog_rejects_unknown_plugins() {
    let error = BundledPagePluginCatalog.load("unknown").unwrap_err();
    assert!(error.to_string().contains("Unknown page plugin"));
}
