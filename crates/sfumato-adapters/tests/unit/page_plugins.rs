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
    assert_eq!(ids, vec!["lottie", "motion", "theatre", "threejs"]);
}

#[test]
fn bundled_catalog_rejects_unknown_plugins() {
    let error = BundledPagePluginCatalog.load("unknown").unwrap_err();
    assert!(error.to_string().contains("Unknown page plugin"));
}
