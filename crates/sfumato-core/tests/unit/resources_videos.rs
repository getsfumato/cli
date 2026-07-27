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
