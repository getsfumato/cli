use super::*;

/// Serves one item's markup, standing in for the installed catalog on disk.
struct CatalogSourceRenderer;

#[async_trait::async_trait]
impl VideoRenderer for CatalogSourceRenderer {
    async fn render(
        &self,
        _engine: VideoEngine,
        _request: &VideoRenderRequest,
        _operation: &OperationContext,
    ) -> crate::errors::SfumatoResult<()> {
        unreachable!("authoring never renders")
    }

    async fn inspect(
        &self,
        _video_path: &Path,
        _operation: &OperationContext,
    ) -> crate::errors::SfumatoResult<VideoInspection> {
        unreachable!("authoring never inspects")
    }

    fn catalog_item_source(
        &self,
        _engine: VideoEngine,
        id: &str,
    ) -> crate::errors::SfumatoResult<String> {
        Ok(format!(
            "<div id=\"{id}\" data-composition-id=\"{id}\" data-width=\"1920\" data-height=\"1080\"></div>"
        ))
    }
}

/// Stands in for an item that is listed but not readable on disk.
struct UnreadableCatalogRenderer;

#[async_trait::async_trait]
impl VideoRenderer for UnreadableCatalogRenderer {
    async fn render(
        &self,
        _engine: VideoEngine,
        _request: &VideoRenderRequest,
        _operation: &OperationContext,
    ) -> crate::errors::SfumatoResult<()> {
        unreachable!("authoring never renders")
    }

    async fn inspect(
        &self,
        _video_path: &Path,
        _operation: &OperationContext,
    ) -> crate::errors::SfumatoResult<VideoInspection> {
        unreachable!("authoring never inspects")
    }
}

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

#[test]
fn asset_retention_reads_every_source_file_not_just_the_entry() {
    // The bug this guards: retention used to run against the plan, so a generated
    // image the author embedded — but the planner never named in its JSON — was
    // deleted from disk before the render. A scene sub-composition is just as
    // likely to carry the reference as the entry composition.
    let source = VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            (
                "index.html".into(),
                "<!DOCTYPE html><html><body><div data-composition-src=\"compositions/scene-1.html\"></div></body></html>"
                    .into(),
            ),
            (
                "compositions/scene-1.html".into(),
                "<img src=\"assets/images/fibre-core.png\" alt=\"core\">".into(),
            ),
        ]),
    )
    .expect("the source is valid");

    let text = source_reference_text(&source);

    assert!(
        text.contains("assets/images/fibre-core.png"),
        "a reference inside a scene composition counts: {text}"
    );
}

#[test]
fn asset_retention_text_covers_the_entry_composition_too() {
    let source = hyperframe_source(
        "<div id=\"root\" data-composition-id=\"root\" data-width=\"1920\" data-height=\"1080\" data-start=\"0\"><img src=\"assets/images/hero.png\"></div>",
    );

    assert!(source_reference_text(&source).contains("assets/images/hero.png"));
}

fn measurement(at_seconds: f32, ink_ratio: f32, distinct_colours: u32) -> VideoFrameMeasurement {
    VideoFrameMeasurement {
        at_seconds,
        ink_ratio,
        distinct_colours,
    }
}

fn timed_plan() -> VideoPlanDocument {
    // Scene starts land on 0, 4, 13, which is the shape of the real 45s video whose
    // every scene boundary rendered empty.
    let response = format!(
        r#"{{"title":"Fibre","objective":"teach","workflow":"explainer","message":"m","narrative_arc":"hook","design_direction":"d","scenes":[{},{},{}],"artifacts":[],"visual_direction":"vd","remote_prompt":""}}"#,
        r#"{"id":"scene-1","start_seconds":0,"duration_seconds":4,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"hook"}}"#,
        r#"{"id":"scene-2","start_seconds":4,"duration_seconds":9,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"body"}}"#,
        r#"{"id":"scene-3","start_seconds":13,"duration_seconds":10,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"payoff"}}"#,
    );
    parse_plan(
        &response,
        VideoEngine::Hyperframe,
        23,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        None,
    )
    .expect("plan parses")
    .0
}

#[test]
fn a_scene_that_opens_on_an_empty_frame_is_a_defect() {
    // The measured reality of a real video: every scene start was under 0.008 ink
    // while every mid-scene frame was over 0.05.
    let plan = timed_plan();
    let measurements = vec![
        measurement(2.0, 0.2998, 35),
        measurement(4.0, 0.0071, 2),
        measurement(8.5, 0.2336, 34),
        measurement(13.0, 0.0, 1),
    ];

    let defects = classify_frames(&plan, &measurements);

    assert_eq!(defects.len(), 2, "both scene starts are reported: {defects:?}");
    assert_eq!(defects[0].scene, Some(2));
    assert_eq!(defects[0].kind, VideoFrameDefectKind::EmptySceneStart);
    assert_eq!(defects[1].scene, Some(3));
    assert!(describe_frame_defect(&defects[1]).contains("scene 3"));
}

#[test]
fn a_sparse_frame_inside_a_scene_is_left_alone() {
    // Holding on a single word over a plain ground is a real choice, so only a
    // frame with literally nothing on it counts away from a scene boundary.
    let plan = timed_plan();
    let measurements = vec![measurement(8.5, 0.004, 2), measurement(18.0, 0.0, 1)];

    let defects = classify_frames(&plan, &measurements);

    assert_eq!(defects.len(), 1, "only the blank one: {defects:?}");
    assert_eq!(defects[0].kind, VideoFrameDefectKind::BlankFrame);
    assert_eq!(defects[0].scene, None);
}

#[test]
fn a_film_whose_frames_all_carry_content_reports_nothing() {
    let plan = timed_plan();
    let measurements = vec![
        measurement(2.0, 0.30, 35),
        measurement(4.0, 0.12, 20),
        measurement(13.0, 0.08, 18),
    ];

    assert!(classify_frames(&plan, &measurements).is_empty());
}

#[test]
fn a_validation_failure_names_the_scene_it_points_at() {
    // Both the core validator and the renderer's check quote the offending path,
    // so the scene can be recovered instead of guessed. Getting this right is what
    // lets one scene be re-authored instead of the whole film being patched.
    let source = VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            (
                "index.html".into(),
                "<!DOCTYPE html><html><body><div data-composition-src=\"compositions/scene-2.html\"></div></body></html>".into(),
            ),
            ("compositions/scene-1.html".into(), "<template><div data-composition-id=\"scene-1\"></div></template>".into()),
            ("compositions/scene-2.html".into(), "<template><div data-composition-id=\"scene-2\"></div></template>".into()),
        ]),
    )
    .expect("source is valid");
    let error = SfumatoError::render(
        ErrorClass::InvalidOutput,
        "missing_or_empty_sub_composition: references \"compositions/scene-2.html\", but the file has no content",
    );

    assert_eq!(failing_scene(&source, &error).as_deref(), Some("scene-2"));
}

#[test]
fn a_failure_that_names_no_scene_falls_back_to_the_whole_film() {
    // A repair path that guessed a scene here would rewrite an innocent one, so
    // naming nothing has to stay distinguishable from naming something.
    let source = VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            (
                "index.html".into(),
                "<!DOCTYPE html><html><body><div data-composition-src=\"compositions/scene-1.html\"></div></body></html>".into(),
            ),
            ("compositions/scene-1.html".into(), "<template><div data-composition-id=\"scene-1\"></div></template>".into()),
        ]),
    )
    .expect("source is valid");
    let error = SfumatoError::render(ErrorClass::Unavailable, "Chrome could not start");

    assert!(failing_scene(&source, &error).is_none());
}

#[test]
fn a_frame_is_attributed_to_the_scene_it_falls_inside() {
    // Scene boundaries decide the label a reviewer sees, and a finding attributed
    // to the wrong scene sends a repair at the wrong file.
    let plan = timed_plan();

    assert_eq!(scene_at(&plan, 0.0), Some(1));
    assert_eq!(scene_at(&plan, 3.9), Some(1));
    // Exactly on a boundary belongs to the scene that is starting, not the one
    // that just ended.
    assert_eq!(scene_at(&plan, 4.0), Some(2));
    assert_eq!(scene_at(&plan, 13.0), Some(3));
    assert_eq!(scene_at(&plan, 23.0), None, "past the end of the film");
}

#[test]
fn a_visual_review_verdict_parses_from_the_contract_the_prompt_asks_for() {
    let approved: VideoVisualReport =
        serde_json::from_str(r#"{"approved": true, "findings": []}"#).expect("the sound case");
    assert!(approved.approved);
    assert!(approved.findings.is_empty());

    // `findings` is optional: a model that approves often omits it entirely, and
    // rejecting that answer would throw away a valid verdict.
    let terse: VideoVisualReport =
        serde_json::from_str(r#"{"approved": true}"#).expect("findings may be omitted");
    assert!(terse.findings.is_empty());

    let rejected: VideoVisualReport = serde_json::from_str(
        r#"{"approved": false, "findings": ["At 13.00s the title is clipped by the right edge"]}"#,
    )
    .expect("the defective case");
    assert!(!rejected.approved);
    assert_eq!(rejected.findings.len(), 1);

    // Prose instead of the object has to fail rather than read as approval.
    serde_json::from_str::<VideoVisualReport>("The frames look fine to me.")
        .expect_err("an unparseable answer is not an approval");
}

#[test]
fn a_selected_catalog_piece_reaches_the_author_as_source_to_adapt() {
    // These files are showcase documents: `flowchart` asks "Should I learn to code?"
    // on unrelated content. Mounting one put that copy into a film about fibre
    // optics, and the author — forbidden to edit a mounted block — hid it under its
    // own ground until the renderer rejected the scene. The technique travels now,
    // not the markup.
    let catalog = managed_catalog();
    let (plan, _) = parse_plan(
        &plan_response(r#"["flowchart"]"#, 20.0),
        VideoEngine::Hyperframe,
        20,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        Some(&catalog),
    )
    .expect("plan parses");
    let renderer = CatalogSourceRenderer;

    let references = scene_catalog_references(
        &plan.scenes()[0],
        Some(&catalog),
        &renderer,
        VideoEngine::Hyperframe,
    );

    assert_eq!(references.len(), 1);
    assert_eq!(references[0].id, "flowchart");
    assert!(references[0].source.contains("data-composition-id"));
}

#[test]
fn a_reference_the_author_could_not_read_is_dropped_rather_than_named() {
    // An example nobody can see is worse than no example: the beat is still
    // authorable by hand, and naming a file the prompt does not carry invites the
    // author to mount a path instead of building the technique.
    let catalog = managed_catalog();
    let (plan, _) = parse_plan(
        &plan_response(r#"["flowchart"]"#, 20.0),
        VideoEngine::Hyperframe,
        20,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        Some(&catalog),
    )
    .expect("plan parses");

    let references = scene_catalog_references(
        &plan.scenes()[0],
        Some(&catalog),
        &UnreadableCatalogRenderer,
        VideoEngine::Hyperframe,
    );

    assert!(references.is_empty());
}

#[test]
fn a_failure_that_names_only_an_element_still_finds_its_scene() {
    // The renderer's legibility errors — overflowing, occluded and overlapping text
    // — quote the element, not the file. A real film reached the whole-film patch
    // for exactly this and then blew past the model's output limit, so the defects
    // that decide whether a video is readable were the ones repair could not route.
    let source = VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            (
                "index.html".into(),
                "<!DOCTYPE html><html><body><div data-composition-src=\"compositions/scene-2.html\"></div></body></html>".into(),
            ),
            ("compositions/scene-1.html".into(), "<template><div data-composition-id=\"scene-1\"></div></template>".into()),
            ("compositions/scene-2.html".into(), "<template><div data-composition-id=\"scene-2\"></div></template>".into()),
        ]),
    )
    .expect("source is valid");
    let error = SfumatoError::render(
        ErrorClass::InvalidOutput,
        "text_box_overflow div.formula inside #scene-2-formula overflowed right 11.58px",
    );

    assert_eq!(failing_scene(&source, &error).as_deref(), Some("scene-2"));
}
