use super::*;

#[test]
fn managed_renderer_manifest_pins_supported_packages() {
    let hyperframe = renderer_package("hyperframe").unwrap();
    let manim = renderer_package("manim").unwrap();

    assert_eq!(hyperframe.package, "hyperframes");
    assert_eq!(hyperframe.version, "0.7.62");
    assert_eq!(
        hyperframe.runtime_packages.get("gsap").map(String::as_str),
        Some("3.15.0")
    );
    assert_eq!(manim.package, "manim");
    assert_eq!(manim.version, "0.20.1");
    assert!(renderer_package("unknown").is_err());
}

#[test]
fn hyperframes_optional_capabilities_do_not_make_the_renderer_unhealthy() {
    let report: HyperframesDoctorReport = serde_json::from_value(serde_json::json!({
        "ok": false,
        "checks": [
            { "name": "Node.js", "ok": true, "detail": "v22" },
            { "name": "FFmpeg", "ok": true, "detail": "installed" },
            { "name": "FFprobe", "ok": true, "detail": "installed" },
            { "name": "Chrome", "ok": true, "detail": "installed" },
            {
                "name": "TTS (Kokoro)",
                "ok": false,
                "detail": "Not installed",
                "hint": "pip install kokoro-onnx"
            },
            { "name": "Docker running", "ok": false, "detail": "Not running" }
        ]
    }))
    .unwrap();

    let (healthy, details) = evaluate_hyperframes_doctor(report);

    assert!(healthy);
    assert_eq!(details.len(), 1);
    assert_eq!(
        details[0],
        "optional capabilities unavailable: TTS (Kokoro), Docker running"
    );
}

#[test]
fn hyperframes_missing_required_dependency_is_unhealthy() {
    let report: HyperframesDoctorReport = serde_json::from_value(serde_json::json!({
        "checks": [
            { "name": "Node.js", "ok": true, "detail": "v22" },
            { "name": "FFmpeg", "ok": true, "detail": "installed" },
            { "name": "FFprobe", "ok": true, "detail": "installed" },
            { "name": "Chrome", "ok": false, "detail": "Not found" }
        ]
    }))
    .unwrap();

    let (healthy, details) = evaluate_hyperframes_doctor(report);

    assert!(!healthy);
    assert_eq!(details, vec!["required Chrome unavailable: Not found"]);
}

#[test]
fn the_bundled_catalog_manifest_is_internally_consistent() {
    let catalog = ManagedVideoRenderers::parse_catalog().expect("bundled manifest parses");
    let mut seen = std::collections::BTreeSet::new();

    for item in catalog.items() {
        assert!(
            seen.insert(item.id.clone()),
            "duplicate catalog id {}",
            item.id
        );
        assert!(
            !item.summary.trim().is_empty(),
            "{} needs a summary",
            item.id
        );
    }
    assert!(!catalog.items().is_empty());
}

#[test]
fn catalog_items_map_to_the_paths_the_renderer_writes() {
    // Guards the copy step: a wrong path silently stages nothing and the
    // generated composition then references a file that is not there.
    let renderers = ManagedVideoRenderers::new(PathBuf::from("/managed"));
    let catalog = ManagedVideoRenderers::parse_catalog().unwrap();
    let block = catalog
        .items()
        .iter()
        .find(|item| item.kind == VideoCatalogKind::Block)
        .unwrap();
    let component = catalog
        .items()
        .iter()
        .find(|item| item.kind == VideoCatalogKind::Component)
        .unwrap();

    assert_eq!(
        renderers.hyperframe_catalog_item(block),
        PathBuf::from(format!(
            "/managed/hyperframe/catalog/compositions/{}.html",
            block.id
        ))
    );
    assert_eq!(
        renderers.hyperframe_catalog_item(component),
        PathBuf::from(format!(
            "/managed/hyperframe/catalog/compositions/components/{}.html",
            component.id
        ))
    );
    // Everything stays under Sfumato's managed root, never in a repository.
    assert!(
        renderers
            .hyperframe_catalog_root()
            .starts_with("/managed/hyperframe")
    );
}

#[test]
#[ignore = "queries the live Hyperframe registry over the network"]
fn every_curated_catalog_id_exists_in_the_upstream_registry() {
    // The failure this catches is real: the previous manifest shipped
    // `lower-third-clean-bar`, which the registry does not have.
    let manifest = std::process::Command::new("curl")
        .args([
            "-sSf",
            "https://raw.githubusercontent.com/heygen-com/hyperframes/main/registry/registry.json",
        ])
        .output()
        .expect("curl runs");
    assert!(manifest.status.success(), "registry fetch failed");
    let registry: serde_json::Value = serde_json::from_slice(&manifest.stdout).unwrap();
    let names = registry["items"]
        .as_array()
        .expect("registry lists items")
        .iter()
        .filter_map(|item| item["name"].as_str().map(ToOwned::to_owned))
        .collect::<std::collections::BTreeSet<_>>();

    let missing = ManagedVideoRenderers::parse_catalog()
        .unwrap()
        .items()
        .iter()
        .filter(|item| !names.contains(&item.id))
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();

    assert!(missing.is_empty(), "ids absent upstream: {missing:?}");
}

#[test]
fn staging_points_a_catalog_item_at_the_pinned_runtime() {
    // The registry pins its own GSAP release, so the CDN URL would both require
    // the network and run a second GSAP beside the one Sfumato vendors.
    let item = r#"<script src="https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js"></script>"#;

    let staged = offline_catalog_item("data-chart", item).unwrap();

    assert_eq!(staged, r#"<script src="vendor/gsap.min.js"></script>"#);
}

#[test]
fn staging_drops_remote_font_links_but_keeps_local_ones() {
    let item = concat!(
        "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\" />\n",
        "<link\n  href=\"https://fonts.googleapis.com/css2?family=Inter&display=block\"\n  rel=\"stylesheet\"\n/>\n",
        "<link rel=\"stylesheet\" href=\"assets/theme.css\" />"
    );

    let staged = offline_catalog_item("morph-text", item).unwrap();

    assert!(!staged.contains("fonts.googleapis.com"));
    assert!(staged.contains("assets/theme.css"));
}

#[test]
fn staging_drops_remote_font_imports_but_keeps_local_ones() {
    // Half the catalog declares its fonts as an `@import` inside `<style>`
    // rather than a `<link>`, so stripping only the markup form leaves it online.
    let item = concat!(
        "<style>\n",
        "  @import url(\"https://fonts.googleapis.com/css2?family=Lato&display=block\");\n",
        "  @import url(\"assets/local.css\");\n",
        "</style>"
    );

    let staged = offline_catalog_item("flash-through-white", item).unwrap();

    assert!(!staged.contains("fonts.googleapis.com"));
    assert!(staged.contains("assets/local.css"));
}

#[test]
fn staging_keeps_svg_namespaces_which_are_never_fetched() {
    let item = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" /></svg>"#;

    assert_eq!(offline_catalog_item("flowchart", item).unwrap(), item);
}

#[test]
fn staging_refuses_a_catalog_item_with_an_unvendorable_reference() {
    // A catalog bump that introduces a new remote dependency has to fail loudly
    // here; the alternative is a render that quietly reaches the network.
    let item = r#"<script src="https://example.com/lib.js"></script>"#;

    let error = offline_catalog_item("data-chart", item).unwrap_err().to_string();

    assert!(error.contains("https://example.com/lib.js"), "{error}");
    assert!(error.contains("data-chart"), "{error}");
}

#[test]
fn every_installed_catalog_item_stages_offline() {
    // Runs against the real installation when present: the registry ships items
    // as standalone documents, and only the staged copy is held to the contract.
    let renderers = match ManagedVideoRenderers::default_path() {
        Ok(renderers) if renderers.hyperframe_catalog_root().is_dir() => renderers,
        _ => return,
    };
    let catalog = ManagedVideoRenderers::parse_catalog().unwrap();

    for item in catalog.items() {
        let installed = renderers.hyperframe_catalog_item(item);
        if !installed.is_file() {
            continue;
        }
        let content = fs::read_to_string(&installed).unwrap();
        let staged = offline_catalog_item(&item.id, &content)
            .unwrap_or_else(|error| panic!("{} does not stage offline: {error}", item.id));
        assert_eq!(first_remote_reference(&staged), None);
    }
}

#[test]
fn staging_wraps_a_component_snippet_so_it_can_be_mounted() {
    // `data-composition-src` renders nothing for a bare snippet, and the author
    // is given item IDs rather than contents, so it cannot paste one in by hand.
    let staged = mountable_component("vignette", "<div id=\"hf-vignette\"></div>", 1280, 720);

    assert!(staged.starts_with("<template>"));
    assert!(staged.contains(r#"data-composition-id="vignette""#));
    assert!(staged.contains(r#"data-width="1280""#));
    assert!(staged.contains(r#"data-height="720""#));
    assert!(staged.contains("hf-vignette"));
}

#[test]
fn staging_leaves_a_component_that_is_already_a_document_alone() {
    // Part of the registry's components ship as standalone documents with their
    // own composition root; wrapping those would nest a second one.
    let document = "<html><body><div data-composition-id=\"morph-text\"></div></body></html>";

    assert_eq!(
        mountable_component("morph-text", document, 1920, 1080),
        document
    );
}

#[test]
fn staging_refuses_a_catalog_item_that_loads_its_own_data_at_render_time() {
    // Why the two caption styles are not curated: they load their words from a
    // sidecar file with per-word timings, which only a narration track can supply
    // and this engine is silent. Re-adding one must fail here, with a reason,
    // rather than surfacing as an unexplained renderer failure mid-render.
    let item = r#"<script>fetch("./caption-data.json").then(function (r) { return r.json(); });</script>"#;

    let error = offline_catalog_item("caption-weight-shift", item)
        .unwrap_err()
        .to_string();

    assert!(error.contains("runtime 'fetch(' request"), "{error}");
    assert!(error.contains("caption-weight-shift"), "{error}");
}

#[test]
fn the_curated_catalog_excludes_items_that_cannot_render_silently() {
    // The catalog and the staging guard have to agree: a curated ID that the
    // guard rejects is a render that fails only once a plan happens to select it.
    let excluded = ["caption-editorial-emphasis", "caption-weight-shift"];
    let catalog = ManagedVideoRenderers::parse_catalog().unwrap();

    for id in excluded {
        assert!(
            catalog.find(id).is_none(),
            "{id} needs a narration track to supply its words; it must stay uncurated until the engine has audio"
        );
    }
    // The role those items served still has to be reachable, or the planner has
    // no way to give a beat typographic stress.
    assert!(
        !catalog
            .for_role(sfumato_core::renderers::VideoCatalogRole::Emphasis)
            .is_empty()
    );
}

#[test]
fn the_manifest_pins_the_document_renderer() {
    // The paginator's version decides where pages break, so it is pinned like
    // every other renderer rather than left to whatever npm resolves today.
    let pagedjs = renderer_package("pagedjs").unwrap();

    assert_eq!(pagedjs.package, "pagedjs-cli");
    assert_eq!(pagedjs.version, "0.4.3");
    assert!(pagedjs.runtime_packages.is_empty());
}

#[test]
fn the_document_renderer_installs_under_its_own_managed_prefix() {
    // A shared prefix would let one renderer's dependency tree overwrite
    // another's, which is how a pinned version silently stops being pinned.
    let renderers = ManagedVideoRenderers::new(PathBuf::from("/managed"));

    assert_eq!(
        renderers.pagedjs_executable(),
        PathBuf::from("/managed/pagedjs/node_modules/.bin/pagedjs-cli")
    );
}

#[test]
fn an_unknown_renderer_names_the_ones_that_exist() {
    let error = renderer_package("weasyprint").unwrap_err().to_string();

    assert!(error.contains("hyperframe"), "{error}");
    assert!(error.contains("manim"), "{error}");
    assert!(error.contains("pagedjs"), "{error}");
}
