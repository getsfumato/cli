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
    let renderers = ManagedVideoRenderers::new(
        PathBuf::from("/managed"),
        std::sync::Arc::new(UvPythonRuntime::new(PathBuf::from("/managed-python"))),
    );
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
    let item =
        r#"<script src="https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js"></script>"#;

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

    let error = offline_catalog_item("data-chart", item)
        .unwrap_err()
        .to_string();

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
    let item =
        r#"<script>fetch("./caption-data.json").then(function (r) { return r.json(); });</script>"#;

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
    let renderers = ManagedVideoRenderers::new(
        PathBuf::from("/managed"),
        std::sync::Arc::new(UvPythonRuntime::new(PathBuf::from("/managed-python"))),
    );

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

#[test]
fn a_flat_frame_measures_as_empty_and_a_drawn_one_does_not() {
    let mut flat = image::RgbaImage::new(64, 64);
    for pixel in flat.pixels_mut() {
        *pixel = image::Rgba([250, 246, 220, 255]);
    }
    let (flat_ink, flat_colours) = measure_frame(&flat);

    let mut drawn = flat.clone();
    for x in 8..56 {
        for y in 20..44 {
            drawn.put_pixel(x, y, image::Rgba([20, 90, 120, 255]));
        }
    }
    let (drawn_ink, drawn_colours) = measure_frame(&drawn);

    assert_eq!(flat_ink, 0.0, "a single-colour frame carries no ink");
    assert_eq!(flat_colours, 1);
    assert!(drawn_ink > 0.1, "a drawn shape reads as ink: {drawn_ink}");
    assert!(drawn_colours >= 2);
}

#[test]
fn antialiasing_alone_does_not_read_as_content() {
    // A frame whose only variation is a hair away from the background is empty,
    // and treating it as content would make the blank-frame gate useless.
    let mut nearly_flat = image::RgbaImage::new(32, 32);
    for (index, pixel) in nearly_flat.pixels_mut().enumerate() {
        let nudge = u8::try_from(index % 4).unwrap_or(0);
        *pixel = image::Rgba([250 - nudge, 246, 220, 255]);
    }

    let (ink, _) = measure_frame(&nearly_flat);

    assert_eq!(ink, 0.0, "a near-uniform frame carries no ink: {ink}");
}

#[test]
#[ignore = "measures the snapshots of a real generated video when one is present"]
fn measures_the_real_video_snapshots() {
    // The regression case that started this work: two frames of a real 45s video
    // were completely empty, and both fell on a scene start.
    let root = dirs::home_dir().unwrap().join(
        ".sfumato/Projects/Facultad/resources/videos/el-interior-de-la-fibra-optica/revisions",
    );
    let Ok(revisions) = std::fs::read_dir(&root) else {
        return;
    };
    for revision in revisions.filter_map(Result::ok) {
        let snapshots = revision.path().join("snapshots");
        let Ok(frames) = std::fs::read_dir(&snapshots) else {
            continue;
        };
        let mut names: Vec<_> = frames
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|value| value == "png"))
            .collect();
        names.sort();
        for path in names {
            let decoded = image::open(&path).unwrap().to_rgba8();
            let (ink, colours) = measure_frame(&decoded);
            println!(
                "{:<22} ink={ink:.4} colours={colours}",
                path.file_name().unwrap().to_string_lossy()
            );
        }
    }
}

#[test]
fn a_failed_check_reports_its_errors_rather_than_its_advice() {
    // Reproduces the report shape that made a real failure undebuggable: pages of
    // lint advice about staged catalog items, with the errors last, against a
    // message length cap that kept only the head.
    let mut stdout = String::from("◆ Checking source\nLint\n");
    for index in 0..80 {
        stdout.push_str(&format!(
            "  ⚠ composition_self_attribute_selector: selector {index} will leak to siblings\n"
        ));
        stdout.push_str("  ℹ pointer_events_none: harder to select in the Studio preview\n");
    }
    stdout.push_str(
        "  ✗ root_missing_composition_id: Root composition is missing `data-composition-id`.\n",
    );
    stdout.push_str("  ✗ missing_or_empty_sub_composition: references \"compositions/scene-2.html\", but the file has no content.\n");
    stdout.push_str("  2 error(s), 160 warning(s), 0 info(s)\n");

    let message = check_failure_message(&stdout, "");

    assert!(message.contains("root_missing_composition_id"), "{message}");
    assert!(message.contains("compositions/scene-2.html"), "{message}");
    assert!(message.contains("2 error(s)"));
    assert!(
        !message.contains("pointer_events_none"),
        "advice about unrelated files is dropped: {message}"
    );
    assert!(
        message.len() < 1_000,
        "the message stays inside the length a user actually sees: {} chars",
        message.len()
    );
}

#[test]
fn a_check_that_failed_without_itemising_keeps_its_tail() {
    // Some failures print no cross-marked line at all. The end of the output is
    // where the reason lives, so the head is the wrong thing to keep.
    let stdout = (0..50)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n◇ Check failed: browser could not start\n";

    let message = check_failure_message(&stdout, "");

    assert!(message.contains("browser could not start"), "{message}");
    assert!(
        !message.contains("line 0"),
        "the head is dropped: {message}"
    );
}

#[test]
fn a_failed_check_keeps_the_path_the_error_points_at() {
    // The renderer prints the offending file on its own line under the marker.
    // A real run lost that line to the error filter, so the repair could not tell
    // which scene was at fault and fell back to re-authoring the entire film —
    // which then blew past the connector's output limit.
    let stdout = concat!(
        "◆ Checking source\n",
        "Lint\n",
        "  ✗ font_family_without_font_face: Font family used without @font-face declaration: fira code.\n",
        "    /tmp/job/source/compositions/scene-3.html .m t=0s\n",
        "    Fix: Add an @font-face rule or use a bundled family.\n",
        "  1 error(s), 51 warning(s), 38 info(s)\n",
    );

    let message = check_failure_message(stdout, "");

    assert!(message.contains("fira code"), "{message}");
    assert!(
        message.contains("compositions/scene-3.html"),
        "the repair needs the path to target one scene: {message}"
    );
    assert!(
        !message.contains("Fix:"),
        "the renderer's advice is still dropped: {message}"
    );
}

#[test]
fn a_failed_render_reports_its_errors_rather_than_its_advice() {
    // The render path printed its raw output, and the renderer leads with pages of
    // lint advice about staged catalog items. Against the message length cap that
    // meant a real failure showed only warnings and never said what went wrong.
    let mut stdout = String::from("◆ Rendering source\n");
    for index in 0..60 {
        stdout.push_str(&format!(
            "  ⚠ [compositions/cross-warp-morph.html] composition_self_attribute_selector: selector {index}\n"
        ));
    }
    stdout.push_str("  ✗ text_occluded #node-yes inside #scene-2-formula \"Yes\" — Text is hidden beneath an opaque element.\n");
    stdout.push_str("  1 error(s), 60 warning(s), 0 info(s)\n");

    let message = check_failure_message(&stdout, "");

    assert!(message.contains("text_occluded"), "{message}");
    assert!(message.contains("#scene-2-formula"), "{message}");
    assert!(
        !message.contains("composition_self_attribute_selector"),
        "the advice is dropped: {message}"
    );
}

/// Runs ffmpeg for a fixture, failing the test with its own diagnostics.
#[cfg(feature = "real-renderers")]
async fn ffmpeg_fixture(arguments: &[&str]) {
    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .args(arguments)
        .args(["-loglevel", "error"]);
    let output = command.output().await.expect("ffmpeg should run");
    assert!(
        output.status.success(),
        "fixture ffmpeg failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(feature = "real-renderers")]
async fn probe_seconds(path: &Path) -> f64 {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .await
        .expect("ffprobe should run");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("ffprobe reports a duration")
}

/// Six seconds of video, two 1.5s narration clips at 0s and 3s, one overlay.
///
/// The narration deliberately stops at 4.5s while the picture runs to 6s, which
/// is the ordinary shape of a film whose last beat holds after the voice stops.
#[cfg(feature = "real-renderers")]
async fn mux_fixture(root: &Path) -> ManimManifest {
    std::fs::create_dir_all(root.join("assets/audio")).expect("fixture directories");
    ffmpeg_fixture(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=navy:s=320x180:d=6:r=24",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        root.join("silent.mp4").to_str().unwrap(),
    ])
    .await;
    for (name, frequency) in [("a.m4a", 440), ("b.m4a", 660)] {
        ffmpeg_fixture(&[
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={frequency}:duration=1.5"),
            "-c:a",
            "aac",
            root.join("assets/audio").join(name).to_str().unwrap(),
        ])
        .await;
    }
    ManimManifest {
        scenes: Vec::new(),
        audio: vec![
            ManimAudioEntry {
                reference: "assets/audio/a.m4a".into(),
                start_seconds: 0.0,
            },
            ManimAudioEntry {
                reference: "assets/audio/b.m4a".into(),
                start_seconds: 3.0,
            },
        ],
        captions: None,
    }
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn narration_that_ends_before_the_picture_does_not_truncate_the_film() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = temp.path();
    let manifest = mux_fixture(root).await;
    let staging = root.join(".assembly");
    std::fs::create_dir_all(&staging).expect("staging");
    let output = root.join("out.mp4");

    compose_film(ComposeFilmRequest {
        silent: &root.join("silent.mp4"),
        captions: None,
        manifest: &manifest,
        source_root: root,
        staging: &staging,
        output_path: &output,
        operation: &OperationContext::detached(),
    })
    .await
    .expect("the film should compose");

    // Without padding the mixed audio, `-shortest` ends the film at the last
    // spoken word and silently drops the final beat's frames.
    let seconds = probe_seconds(&output).await;
    assert!(
        (seconds - 6.0).abs() < 0.25,
        "expected the full 6s picture, got {seconds}s"
    );
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn a_film_with_neither_narration_nor_captions_is_copied_untouched() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = temp.path();
    let mut manifest = mux_fixture(root).await;
    manifest.audio.clear();
    let staging = root.join(".assembly");
    std::fs::create_dir_all(&staging).expect("staging");
    let output = root.join("out.mp4");

    compose_film(ComposeFilmRequest {
        silent: &root.join("silent.mp4"),
        captions: None,
        manifest: &manifest,
        source_root: root,
        staging: &staging,
        output_path: &output,
        operation: &OperationContext::detached(),
    })
    .await
    .expect("the film should compose");

    // Nothing to mix and nothing to draw, so re-encoding would only lose quality.
    assert_eq!(
        std::fs::read(root.join("silent.mp4")).unwrap(),
        std::fs::read(&output).unwrap()
    );
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn a_missing_narration_clip_fails_instead_of_rendering_a_silent_film() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = temp.path();
    let mut manifest = mux_fixture(root).await;
    manifest.audio.push(ManimAudioEntry {
        reference: "assets/audio/gone.m4a".into(),
        start_seconds: 5.0,
    });
    let staging = root.join(".assembly");
    std::fs::create_dir_all(&staging).expect("staging");

    let error = compose_film(ComposeFilmRequest {
        silent: &root.join("silent.mp4"),
        captions: None,
        manifest: &manifest,
        source_root: root,
        staging: &staging,
        output_path: &root.join("out.mp4"),
        operation: &OperationContext::detached(),
    })
    .await
    .expect_err("a missing clip is a defect, not a quieter film");
    assert!(format!("{error:#}").contains("gone.m4a"));
}

/// Writes a Manim source tree with one scene module and its manifest.
#[cfg(feature = "real-renderers")]
fn manim_source(root: &Path, module_body: &str) {
    std::fs::create_dir_all(root.join("scenes")).expect("scene directory");
    std::fs::write(root.join("scenes/scene_1.py"), module_body).expect("scene module");
    std::fs::write(
        root.join("manifest.json"),
        serde_json::json!({
            "scenes": [{
                "id": "scene-1",
                "module": "scenes/scene_1.py",
                "class_name": "Scene_scene_1",
                "duration_seconds": 2.0
            }],
            "audio": [],
            "captions": null
        })
        .to_string(),
    )
    .expect("manifest");
}

#[cfg(feature = "real-renderers")]
fn managed_renderers() -> ManagedVideoRenderers {
    ManagedVideoRenderers::new(
        ManagedVideoRenderers::default_root().expect("managed root"),
        Arc::new(UvPythonRuntime::default_path().expect("python root")),
    )
}

#[cfg(feature = "real-renderers")]
fn manim_request(root: &Path) -> VideoRenderRequest {
    VideoRenderRequest {
        source_root: root.to_path_buf(),
        output_path: root.join("out.mp4"),
        duration_seconds: 2,
        width: 854,
        height: 480,
        fps: 24,
        quality: "draft".to_string(),
    }
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn a_scene_that_only_fails_when_it_runs_is_caught_before_the_film_is_rendered() {
    let temp = tempfile::tempdir().expect("temporary root");
    // Exactly the fault that killed a real film: `TransformMatchingTex` reads
    // `tex_string` off both sides, so pairing it with a `Text` parses cleanly and
    // raises only once Manim builds the animation. Caught at render, it wasted a
    // whole authoring and narration pass with nothing able to repair it.
    manim_source(
        temp.path(),
        "from manim import *\n\n\
         class Scene_scene_1(Scene):\n\
         \x20   def construct(self):\n\
         \x20       a = Text(\"hola\")\n\
         \x20       b = MathTex(r\"F(s)\")\n\
         \x20       self.add(a)\n\
         \x20       self.play(TransformMatchingTex(a, b), run_time=1.0)\n\
         \x20       self.wait(1.0)\n",
    );

    let error = managed_renderers()
        .validate(
            VideoEngine::Manim,
            &manim_request(temp.path()),
            &OperationContext::detached(),
        )
        .await
        .expect_err("a scene that raises must not reach the render");
    // The message has to name the module, because that is how the repair loop
    // recovers which scene to re-author.
    assert!(
        format!("{error}").contains("scenes/scene_1.py"),
        "the failure must name its scene: {error}"
    );
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn a_scene_that_runs_passes_validation_and_leaves_no_probe_output() {
    let temp = tempfile::tempdir().expect("temporary root");
    manim_source(
        temp.path(),
        "from manim import *\n\n\
         class Scene_scene_1(Scene):\n\
         \x20   def construct(self):\n\
         \x20       self.play(Write(MathTex(r\"F(s)\")), run_time=1.0)\n\
         \x20       self.wait(1.0)\n",
    );

    managed_renderers()
        .validate(
            VideoEngine::Manim,
            &manim_request(temp.path()),
            &OperationContext::detached(),
        )
        .await
        .expect("a well-formed scene validates");
    assert!(
        !temp.path().join(".dry-run").exists(),
        "the probe's working directory must not survive into the revision"
    );
}
