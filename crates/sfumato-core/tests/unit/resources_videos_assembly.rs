use super::*;

use sfumato_domain::{VideoEngine, VideoWorkflow};

/// Three scenes starting at 0, 4 and 13 seconds, the shape of a real film.
fn plan() -> VideoPlanDocument {
    let response = format!(
        r#"{{"title":"Fibre","objective":"teach","workflow":"explainer","message":"m","narrative_arc":"hook","design_direction":"d","scenes":[{},{},{}],"artifacts":[],"visual_direction":"vd","remote_prompt":""}}"#,
        r#"{"id":"scene-1","start_seconds":0,"duration_seconds":4,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"hook"}}"#,
        r#"{"id":"scene-2","start_seconds":4,"duration_seconds":9,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"body"}}"#,
        r#"{"id":"scene-3","start_seconds":13,"duration_seconds":10.5,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"payoff"}}"#,
    );
    super::super::parse_plan(
        &response,
        VideoEngine::Hyperframe,
        24,
        Some("Fibre"),
        VideoWorkflow::Explainer,
        None,
    )
    .expect("plan parses")
    .0
}

#[test]
fn the_master_composition_satisfies_the_renderer_contract() {
    // Every one of these is a rule the renderer enforces. Generating them removes
    // the whole class of authoring failure that used to need a repair pass.
    let html = master_index_html(&plan(), 1280, 720, &NarrationLayer::default());

    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<html>") && html.contains("<body>"));
    assert!(html.contains("data-composition-id=\"root\""));
    assert!(html.contains("data-width=\"1280\"") && html.contains("data-height=\"720\""));
    assert!(html.contains("./vendor/gsap.min.js"));
    assert!(html.contains("gsap.timeline"));
    assert!(html.contains("window.__timelines"));
    // The validator compacts whitespace before looking for this.
    let compact: String = html
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect();
    assert!(compact.contains("paused:true"));
}

#[test]
fn a_scene_identifier_cannot_close_the_attribute_it_is_written_into() {
    // The domain rejects an ID like this before assembly ever sees it. This asserts
    // the second layer: the markup a headless browser executes escapes what it
    // interpolates, so a quote that got past validation cannot become script.
    let escaped = escape_attribute("a\" onload=\"fetch('http://evil')");
    assert!(
        !escaped.contains('"'),
        "an unescaped quote would end `data-composition-src` and start an attribute"
    );
    assert_eq!(escape_attribute("scene-1"), "scene-1");
    assert_eq!(escape_attribute("a<b&c>d"), "a&lt;b&amp;c&gt;d");
}

#[test]
fn every_planned_scene_is_mounted_at_its_planned_position() {
    let html = master_index_html(&plan(), 1920, 1080, &NarrationLayer::default());

    for (scene, start, duration, track) in [
        ("scene-1", "0", "4", "0"),
        ("scene-2", "4", "9", "1"),
        ("scene-3", "13", "10.5", "2"),
    ] {
        let mount = format!(
            "data-composition-src=\"compositions/{scene}.html\" data-start=\"{start}\" data-duration=\"{duration}\" data-track-index=\"{track}\""
        );
        assert!(html.contains(&mount), "missing mount for {scene}:\n{html}");
    }
}

#[test]
fn a_mount_carries_its_own_identifier_beside_the_source() {
    // Measured against the renderer: a host with only `data-composition-src` is
    // reported as a composition host missing its ID, and the scene never mounts.
    let html = master_index_html(&plan(), 1280, 720, &NarrationLayer::default());

    assert!(
        html.contains("id=\"mount-scene-1\" class=\"clip\" data-composition-id=\"mount-scene-1\"")
    );
}

#[test]
fn the_master_timeline_runs_for_the_requested_duration() {
    let html = master_index_html(&plan(), 1280, 720, &NarrationLayer::default());

    assert!(
        html.contains("tl.set({}, {}, 24);"),
        "the timeline has to reach the requested duration:\n{html}"
    );
}

#[test]
fn a_scene_composition_without_a_template_is_rejected() {
    // A bare element renders nothing when mounted; the renderer reports the
    // sub-composition as empty and the scene silently disappears from the film.
    let error = validate_scene_composition(
        "scene-1",
        "<div data-composition-id=\"scene-1\" data-width=\"1280\" data-height=\"720\"></div>",
    )
    .expect_err("a bare element is invalid");

    assert!(error.contains("<template>"), "{error}");
}

#[test]
fn a_scene_composition_must_carry_its_own_identifier_and_canvas() {
    let missing_id = validate_scene_composition(
        "scene-2",
        "<template><div data-width=\"1280\" data-height=\"720\"></div></template>",
    )
    .expect_err("the root needs its composition ID");
    assert!(
        missing_id.contains("data-composition-id=\"scene-2\""),
        "{missing_id}"
    );

    let missing_canvas = validate_scene_composition(
        "scene-2",
        "<template><div data-composition-id=\"scene-2\"></div></template>",
    )
    .expect_err("the root needs its canvas");
    assert!(missing_canvas.contains("data-width"), "{missing_canvas}");
}

#[test]
fn a_scene_composition_must_register_its_own_timeline() {
    let error = validate_scene_composition(
        "scene-3",
        "<template><div data-composition-id=\"scene-3\" data-width=\"1280\" data-height=\"720\"></div></template>",
    )
    .expect_err("a scene without a timeline never animates");

    assert!(error.contains("window.__timelines"), "{error}");
}

#[test]
fn a_complete_scene_composition_is_accepted() {
    let markup = "<template>\n<div id=\"scene-1\" data-composition-id=\"scene-1\" data-width=\"1280\" data-height=\"720\" data-start=\"0\">\n<h1 id=\"t\" class=\"clip\" data-start=\"0\" data-duration=\"4\" data-track-index=\"0\">Luz</h1>\n</div>\n<script>const tl = gsap.timeline({ paused: true }); window.__timelines = window.__timelines || {}; window.__timelines[\"scene-1\"] = tl;</script>\n</template>";

    assert!(validate_scene_composition("scene-1", markup).is_ok());
}

#[test]
fn a_scene_naming_a_font_the_renderer_cannot_supply_is_rejected() {
    // The exact stack a real film shipped. `hyperframes check` failed it with
    // `font_family_without_font_face`, because it resolves fallbacks too, and the
    // whole render was lost to a font nothing would even have displayed.
    let markup = "<template><div data-composition-id=\"scene-1\" data-width=\"1280\" data-height=\"720\"><style>#k{font:500 25px/1 JetBrains Mono,Fira Code,monospace}</style></div><script>window.__timelines={}</script></template>";

    let error = validate_scene_composition("scene-1", markup).expect_err("fira code is unbundled");

    assert!(error.contains("fira code"), "{error}");
    assert!(error.contains("fallbacks included"), "{error}");
}

#[test]
fn a_scene_using_bundled_and_substituted_families_is_accepted() {
    // `Arial` is not bundled but the renderer substitutes it, and the default
    // theme ships exactly this stack: rejecting it would fight the theme.
    let markup = "<template><div data-composition-id=\"scene-2\" data-width=\"1280\" data-height=\"720\"><style>#root{font-family:Inter, Arial, sans-serif}.m{font-family:'JetBrains Mono',monospace}h1{font-size:62px;font-weight:650}</style></div><script>window.__timelines={}</script></template>";

    assert_eq!(validate_scene_composition("scene-2", markup), Ok(()));
}

#[test]
fn the_font_gate_reads_svg_attributes_and_ignores_other_font_properties() {
    let attribute = "<template><div data-composition-id=\"s\" data-width=\"1\" data-height=\"1\"><text font-family=\"Comic Sans MS\">x</text></div><script>window.__timelines={}</script></template>";
    let error =
        validate_scene_composition("s", attribute).expect_err("the attribute spelling counts");
    assert!(error.contains("comic sans ms"), "{error}");

    // `font-size` and `font-weight` name no family and must not be misread as one.
    let sizes = "<template><div data-composition-id=\"s\" data-width=\"1\" data-height=\"1\"><style>h1{font-size:62px;font-weight:650;font-style:italic}</style></div><script>window.__timelines={}</script></template>";
    assert_eq!(validate_scene_composition("s", sizes), Ok(()));
}

#[test]
fn a_quoted_first_family_does_not_hide_the_rest_of_the_stack() {
    // Quotes mean opposite things in the two spellings. Reading the CSS form as if
    // the quote closed the value stopped the scan after the first family, so an
    // unbundled font later in the same stack went unnoticed and the render failed.
    let markup = "<template><div data-composition-id=\"s\" data-width=\"1\" data-height=\"1\"><style>.m{font-family:'JetBrains Mono', Fira Code, monospace}</style></div><script>window.__timelines={}</script></template>";

    let error = validate_scene_composition("s", markup).expect_err("the tail of the stack counts");

    assert!(error.contains("fira code"), "{error}");
}

#[test]
fn an_unquoted_svg_font_attribute_ends_at_the_tag_and_not_mid_document() {
    // Reading past the tag would swallow the rest of the markup as one family name.
    let markup = "<template><div data-composition-id=\"s\" data-width=\"1\" data-height=\"1\"><text font-family=Inter>x</text><text font-family=Comic>y</text></div><script>window.__timelines={}</script></template>";

    let error = validate_scene_composition("s", markup).expect_err("the second attribute counts");

    assert!(
        error.contains("comic"),
        "the first attribute must not swallow the second: {error}"
    );
}

#[test]
fn a_font_declared_through_a_custom_property_is_not_a_missing_family() {
    // The scene prompt asks authors to theme through semantic variables, and the
    // gate rejected them: a real re-authored scene was thrown away for writing
    // `font-family: var(--font-body)`, which names no font at all.
    let markup = "<template><div data-composition-id=\"s\" data-width=\"1\" data-height=\"1\"><style>#root{font-family:var(--font-body, sans-serif)}</style></div><script>window.__timelines={}</script></template>";

    assert_eq!(validate_scene_composition("s", markup), Ok(()));
}

#[test]
fn a_narrated_film_mounts_its_audio_at_the_composition_root() {
    // The renderer only decodes media that is a direct child of the host root, so
    // a narration file nested inside a scene would render silently.
    let layer = NarrationLayer {
        clips: vec![
            NarrationClip {
                reference: "assets/audio/narration-scene-1.mp3".into(),
                start_seconds: 0.0,
                duration_seconds: 3.5,
            },
            NarrationClip {
                reference: "assets/audio/narration-scene-2.mp3".into(),
                start_seconds: 4.0,
                duration_seconds: 8.0,
            },
        ],
        captions: true,
    };
    let html = master_index_html(&plan(), 1280, 720, &layer);

    // No data-duration: the renderer derives an audio clip's end from the file
    // itself and rejects the element when both are present.
    assert!(html.contains(
        "<audio id=\"narration-0\" class=\"clip\" src=\"assets/audio/narration-scene-1.mp3\" data-start=\"0\" data-track-index=\"4\" data-volume=\"1\">"
    ), "{html}");
    assert!(!html.contains("<audio id=\"narration-0\" class=\"clip\" src=\"assets/audio/narration-scene-1.mp3\" data-start=\"0\" data-duration"));
    assert!(html.contains("src=\"assets/audio/narration-scene-2.mp3\" data-start=\"4\""));
    // Captions ride above every scene, and audio above the captions.
    assert!(html.contains("data-composition-src=\"compositions/captions.html\" data-start=\"0\" data-duration=\"24\" data-track-index=\"3\""), "{html}");
}

#[test]
fn a_silent_film_mounts_neither_audio_nor_captions() {
    let html = master_index_html(&plan(), 1280, 720, &NarrationLayer::default());

    assert!(!html.contains("<audio"));
    assert!(!html.contains("captions.html"));
}

#[test]
fn the_caption_overlay_tracks_the_words_that_were_spoken() {
    let groups = crate::resources::narration::caption_groups(
        &[
            crate::providers::SpeechWordTiming {
                text: "light".into(),
                start_seconds: 0.0,
                end_seconds: 0.4,
            },
            crate::providers::SpeechWordTiming {
                text: "travels.".into(),
                start_seconds: 0.4,
                end_seconds: 0.9,
            },
        ],
        2.0,
    );
    let html = captions_composition_html(&groups, 1920, 1080);

    assert!(html.contains("<template>"));
    assert!(html.contains("data-composition-id=\"captions\""));
    assert!(html.contains("data-width=\"1920\"") && html.contains("data-height=\"1080\""));
    assert!(html.contains("window.__timelines[\"captions\"]"));
    // Placed where the voice is, not where the plan guessed.
    assert!(html.contains("data-start=\"2\""), "{html}");
    assert!(html.contains("light travels."));
    // A scrim rather than a shadow alone: the film's own palette decides what is
    // behind a caption, and white-on-cream is exactly the case a shadow loses.
    assert!(
        html.contains("<span class=\"caption-line\">light travels.</span>"),
        "{html}"
    );
    assert!(
        html.contains("background: rgba(12, 14, 17, 0.66)"),
        "{html}"
    );
    // A group that never hard-stops stacks over the next one.
    assert!(
        html.contains("tl.set(\"#caption-0\", { opacity: 0 }, 2.9);"),
        "{html}"
    );
}

#[test]
fn caption_text_cannot_inject_markup() {
    let groups = vec![crate::resources::narration::CaptionGroup {
        text: "a < b & <script>".into(),
        start_seconds: 0.0,
        end_seconds: 1.0,
        words: Vec::new(),
    }];
    let html = captions_composition_html(&groups, 1280, 720);

    assert!(html.contains("a &lt; b &amp; &lt;script&gt;"), "{html}");
    assert!(!html.contains("<script>a"), "{html}");
}
