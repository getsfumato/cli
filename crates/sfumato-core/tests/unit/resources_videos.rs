use super::*;

fn hyperframe_source(html: &str) -> VideoSourceDocument {
    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"UTF-8\"></head><body>{html}</body></html>"
    );
    VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            ("index.html".into(), html),
            (
                "compositions/scene-1.html".into(),
                "<div id=\"scene-1\">Local scene composition</div>".into(),
            ),
        ]),
    )
    .unwrap()
}

#[test]
fn accepts_a_minimal_offline_hyperframe_project() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><!-- compositions/scene-1.html --><div id="title" class="clip" data-start="0" data-duration="10" data-track-index="0"></div><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({ paused: true }); tl.set({}, {}, 10); window.__timelines = window.__timelines || {}; window.__timelines["root"] = tl;</script></div>"#,
    );

    validate_source(&source).unwrap();
}

#[test]
fn accepts_compact_paused_timeline_syntax() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><!-- compositions/scene-1.html --><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({paused:true}); window.__timelines = { root: tl };</script></div>"#,
    );

    validate_source(&source).unwrap();
}

#[test]
fn rejects_hyperframe_source_without_composition_metadata() {
    let source = hyperframe_source("lesson");

    let error = validate_source(&source).unwrap_err();

    assert!(error.to_string().contains("data-composition-id"));
}

#[test]
fn requires_modular_local_compositions_for_new_hyperframe_source() {
    let source = VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            ("index.html".into(), r#"<!DOCTYPE html><html><body><div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({ paused: true }); window.__timelines = { root: tl };</script></div></body></html>"#.into()),
        ]),
    )
    .unwrap();

    assert!(validate_source(&source).is_err());
}

#[test]
fn normalizes_parent_assets_from_direct_compositions_to_project_root_paths() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><!-- compositions/scene-1.html --><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({ paused: true }); window.__timelines = { root: tl };</script></div>"#,
    );
    let mut files = source.files().clone();
    files.insert(
        "compositions/scene-1.html".into(),
        r#"<img src="../assets/fiber.png"><script src="../vendor/gsap.min.js"></script>"#.into(),
    );
    let source = normalize_hyperframe_parent_paths(
        VideoSourceDocument::new(VideoEngine::Hyperframe, files).unwrap(),
    )
    .unwrap();

    validate_source(&source).unwrap();
    let composition = &source.files()["compositions/scene-1.html"];
    assert!(composition.contains(r#"src="assets/fiber.png""#));
    assert!(composition.contains(r#"src="vendor/gsap.min.js""#));
    assert!(!composition.contains("../"));
}

#[test]
fn normalizes_fragment_entrypoint_into_a_complete_html_document() {
    let source = VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            (
                "index.html".into(),
                r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><!-- compositions/scene-1.html --><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({ paused: true }); window.__timelines = { root: tl };</script></div>"#.into(),
            ),
            (
                "compositions/scene-1.html".into(),
                "<div>Scene</div>".into(),
            ),
        ]),
    )
    .unwrap();

    let source = normalize_hyperframe_parent_paths(source).unwrap();
    validate_source(&source).unwrap();
    assert!(source.files()["index.html"].starts_with("<!DOCTYPE html>"));
}

#[test]
fn rejects_parent_escape_hidden_after_a_managed_asset_prefix() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><!-- compositions/scene-1.html --><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({ paused: true }); window.__timelines = { root: tl };</script></div>"#,
    );
    let mut files = source.files().clone();
    files.insert(
        "compositions/scene-1.html".into(),
        r#"<img src="../assets/../../secrets.txt">"#.into(),
    );
    let source = VideoSourceDocument::new(VideoEngine::Hyperframe, files).unwrap();

    assert!(normalize_hyperframe_parent_paths(source).is_err());
}

#[test]
fn rejects_network_access_in_local_video_source() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><script src="./vendor/gsap.min.js"></script><script>fetch('https://example.com'); const tl = gsap.timeline({ paused: true }); window.__timelines["root"] = tl;</script></div>"#,
    );

    assert!(validate_source(&source).is_err());
}

#[test]
fn accepts_standard_inline_svg_namespace_without_treating_it_as_network_access() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><!-- compositions/scene-1.html --><svg xmlns="http://www.w3.org/2000/svg"></svg><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({ paused: true }); window.__timelines = { root: tl };</script></div>"#,
    );

    validate_source(&source).unwrap();
}

#[test]
fn publishes_only_the_processed_mp4_under_the_sfumato_namespace() {
    assert_eq!(
        video_publish_destination(Path::new("/vault/course"), "fourier-series"),
        PathBuf::from("/vault/course/_sfumato/videos/fourier-series")
    );
}

#[test]
fn approval_reuses_the_destination_saved_with_the_review() {
    assert_eq!(
        review_publish_root(Some(PathBuf::from("/course")), None, false),
        Some(PathBuf::from("/course"))
    );
}

#[test]
fn approval_out_override_wins_over_the_saved_destination() {
    assert_eq!(
        review_publish_root(
            Some(PathBuf::from("/course")),
            Some(PathBuf::from("/exports")),
            true,
        ),
        Some(PathBuf::from("/exports"))
    );
}

#[test]
fn legacy_review_records_without_a_publish_root_remain_readable() {
    let record: ReviewSessionRecord = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "review_id": "lesson-review",
            "project": "Course",
            "engine": "hyperframe",
            "status": "pending_approval",
            "resolution": "1080p",
            "aspect_ratio": "16:9",
            "fps": 30,
            "quality": "high",
            "source_hash": "source",
            "plan_hash": "plan"
        }"#,
    )
    .unwrap();

    assert_eq!(record.publish_root, None);
}

#[test]
fn computes_even_video_dimensions_from_resolution_and_aspect_ratio() {
    assert_eq!(
        resolution_dimensions("1080p", "16:9").unwrap(),
        (1920, 1080)
    );
    assert_eq!(resolution_dimensions("720p", "9:16").unwrap(), (406, 720));
}

use crate::renderers::{VideoCatalogKind, VideoCatalogRole};

/// The catalog the workflow ships, so tests assert against real metadata.
fn managed_catalog() -> VideoCatalog {
    VideoCatalog::parse(include_str!(
        "../../../sfumato-adapters/assets/video-catalog/manifest.json"
    ))
    .expect("bundled catalog manifest parses")
}

fn plan_response(catalog_items: &str, duration_seconds: f32) -> String {
    format!(
        r#"{{"title":"Fibre","objective":"teach","workflow":"explainer","message":"m","narrative_arc":"hook","design_direction":"d","scenes":[{{"id":"scene-1","start_seconds":0,"duration_seconds":{duration_seconds},"content":"c","visual":"v","artifacts":[],"production":{{"narrative_role":"hook","catalog_items":{catalog_items}}}}}],"artifacts":[],"visual_direction":"vd","remote_prompt":""}}"#
    )
}

#[test]
fn catalog_summary_is_generated_from_the_installed_manifest() {
    // A hand-written list drifts from what is installed, and the planner is told
    // to select only catalog IDs, so drift becomes a broken composition.
    let summary = managed_catalog().summary();

    assert!(summary.contains("sfumato-hyperframe-2"));
    assert!(summary.contains("flowchart"));
    // Durations reach the planner because a scene must outlast its block.
    assert!(
        summary.contains("15s"),
        "block runtimes are shown: {summary}"
    );
    for role in [
        "process",
        "quantity",
        "code",
        "emphasis",
        "transition",
        "grade",
    ] {
        assert!(summary.contains(role), "role {role} missing from {summary}");
    }
}

#[test]
fn every_curated_role_offers_at_least_one_item() {
    let catalog = managed_catalog();

    for role in [
        VideoCatalogRole::Process,
        VideoCatalogRole::Quantity,
        VideoCatalogRole::Code,
        VideoCatalogRole::Emphasis,
        VideoCatalogRole::Transition,
        VideoCatalogRole::Grade,
    ] {
        assert!(
            !catalog.for_role(role).is_empty(),
            "role {} has no items, so the planner cannot serve that beat",
            role.as_str()
        );
    }
}

#[test]
fn blocks_declare_a_runtime_and_components_do_not() {
    for item in managed_catalog().items() {
        match item.kind {
            VideoCatalogKind::Block => assert!(
                item.duration_seconds.is_some(),
                "block {} needs a runtime for the duration invariant",
                item.id
            ),
            // Components are merged snippets with no timeline of their own.
            VideoCatalogKind::Component => assert!(
                item.duration_seconds.is_none(),
                "component {} must not declare a runtime",
                item.id
            ),
        }
    }
}

#[test]
fn rejects_a_scene_shorter_than_the_block_it_selected() {
    // data-chart is authored for 15s; a 6s scene truncates its reveal.
    let violations = managed_catalog().validate_selection("scene-1", 6.0, &["data-chart".into()]);

    assert_eq!(violations.len(), 1);
    assert!(matches!(
        violations[0],
        VideoCatalogViolation::SceneTooShort { .. }
    ));
    assert!(violations[0].to_string().contains("cut off"));
}

#[test]
fn accepts_a_scene_that_outlasts_its_block() {
    assert!(
        managed_catalog()
            .validate_selection("scene-1", 15.0, &["data-chart".into()])
            .is_empty()
    );
}

#[test]
fn drops_unknown_and_whole_film_selections_from_a_drafted_plan() {
    // `vignette` is a grade item and `us-map` was removed from the catalog: both
    // would reference a composition that is never installed.
    let response = plan_response(r#"["data-chart","vignette","us-map"]"#, 15.0);
    let catalog = managed_catalog();

    let (plan, warnings) = parse_plan(
        &response,
        VideoEngine::Hyperframe,
        20,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        Some(&catalog),
    )
    .expect("plan parses");

    let selected = &plan.scenes()[0].production.catalog_items;
    assert_eq!(selected, &vec!["data-chart".to_string()]);
    assert_eq!(
        warnings.len(),
        2,
        "both removals are reported: {warnings:?}"
    );
    assert!(warnings.iter().any(|warning| warning.contains("us-map")));
    assert!(warnings.iter().any(|warning| warning.contains("vignette")));
}

#[test]
fn a_truncated_reveal_never_costs_the_reviewer_its_patch() {
    // The draft keeps a scene shorter than its block and only warns, so the
    // post-review re-check has to judge it the same way. Treating it as fatal
    // there would discard every unrelated correction the patch carried, on
    // every plan that already shipped the violation.
    let catalog = managed_catalog();
    let (plan, warnings) = parse_plan(
        &plan_response(r#"["data-chart"]"#, 6.0),
        VideoEngine::Hyperframe,
        20,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        Some(&catalog),
    )
    .expect("plan parses");

    assert_eq!(
        plan.scenes()[0].production.catalog_items,
        vec!["data-chart".to_string()],
        "a truncated reveal is kept, not stripped: {warnings:?}"
    );

    let violations = catalog_violations(&plan, Some(&catalog));
    assert_eq!(violations.len(), 1);
    assert!(
        !violations[0].is_unusable(),
        "a truncated reveal still renders, so it must not reject a patch"
    );
    // The draft already reported it, so the post-review pass must recognise the
    // warning it would emit rather than duplicating it.
    assert!(warnings.contains(&catalog_warning(&violations[0])));
}

#[test]
fn an_unknown_selection_introduced_by_review_is_unusable() {
    // Parsed without a catalog so nothing is stripped, which is the state a
    // reviewer patch leaves behind when it names an ID the catalog lacks.
    let (plan, _) = parse_plan(
        &plan_response(r#"["us-map"]"#, 15.0),
        VideoEngine::Hyperframe,
        20,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        None,
    )
    .expect("plan parses");

    let violations = catalog_violations(&plan, Some(&managed_catalog()));

    assert_eq!(violations.len(), 1);
    assert!(violations[0].is_unusable());
}

#[test]
fn keeps_selections_untouched_when_no_catalog_is_available() {
    // Manim and direct models have no registry, so nothing may be stripped.
    let response = plan_response(r#"["anything"]"#, 4.0);

    let (plan, warnings) = parse_plan(
        &response,
        VideoEngine::Manim,
        20,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        None,
    )
    .expect("plan parses");

    assert_eq!(
        plan.scenes()[0].production.catalog_items,
        vec!["anything".to_string()]
    );
    assert!(warnings.is_empty());
}
