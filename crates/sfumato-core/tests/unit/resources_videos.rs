use super::*;

fn hyperframe_source(html: &str) -> VideoSourceDocument {
    VideoSourceDocument::new(
        VideoEngine::Hyperframe,
        BTreeMap::from([
            ("meta.json".into(), r#"{"name":"lesson"}"#.into()),
            ("index.html".into(), html.into()),
        ]),
    )
    .unwrap()
}

#[test]
fn accepts_a_minimal_offline_hyperframe_project() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><div id="title" class="clip" data-start="0" data-duration="10" data-track-index="0"></div><script src="./vendor/gsap.min.js"></script><script>const tl = gsap.timeline({ paused: true }); tl.set({}, {}, 10); window.__timelines = window.__timelines || {}; window.__timelines["root"] = tl;</script></div>"#,
    );

    validate_source(&source).unwrap();
}

#[test]
fn rejects_hyperframe_source_without_composition_metadata() {
    let source = hyperframe_source("<html><body>lesson</body></html>");

    let error = validate_source(&source).unwrap_err();

    assert!(error.to_string().contains("data-composition-id"));
}

#[test]
fn rejects_network_access_in_local_video_source() {
    let source = hyperframe_source(
        r#"<div id="root" data-composition-id="root" data-start="0" data-width="1920" data-height="1080"><script src="./vendor/gsap.min.js"></script><script>fetch('https://example.com'); const tl = gsap.timeline({ paused: true }); window.__timelines["root"] = tl;</script></div>"#,
    );

    assert!(validate_source(&source).is_err());
}

#[test]
fn publishes_only_the_processed_mp4_under_the_sfumato_namespace() {
    assert_eq!(
        video_publish_destination(Path::new("/vault/course"), "fourier-series"),
        PathBuf::from("/vault/course/_sfumato/videos/fourier-series")
    );
}

#[test]
fn computes_even_video_dimensions_from_resolution_and_aspect_ratio() {
    assert_eq!(
        resolution_dimensions("1080p", "16:9").unwrap(),
        (1920, 1080)
    );
    assert_eq!(resolution_dimensions("720p", "9:16").unwrap(), (406, 720));
}
