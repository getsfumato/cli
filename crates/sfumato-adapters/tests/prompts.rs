use std::{fs, path::PathBuf};

use serde_json::{Map, Value, json};
use sfumato_adapters::prompts::LayeredPromptCatalog;
use sfumato_core::prompts::{
    PromptCatalog, PromptId, PromptOrigin, PromptOverrideScope, PromptRenderRequest,
    PromptVariables,
};
use sha2::{Digest, Sha256};

fn representative_variables() -> PromptVariables {
    let mut values = Map::new();
    for (key, value) in [
        ("learning_style", json!(["visual", "step-by-step"])),
        ("project", json!("university")),
        ("project_root", json!("/tmp/university")),
        ("theme_name", json!("gruvbox")),
        (
            "theme_colors",
            json!({"background": "#282828", "text": "#ebdbb2"}),
        ),
        ("theme_fonts", json!({"body": "Inter"})),
        ("instruction", json!("Explain Fourier series visually")),
        ("project_instructions", json!("Teach in Spanish.")),
        ("title", json!("Fourier Series")),
        ("title_provided", json!(true)),
        ("image_generation_available", json!(true)),
        ("narration_available", json!(true)),
        ("video_generation_available", json!(true)),
        (
            "accessible_description",
            json!("Animated Fourier decomposition"),
        ),
        ("engine", json!("hyperframe")),
        ("workflow", json!("explainer")),
        ("urls", json!(["https://example.com/source"])),
        (
            "catalog",
            json!("managed catalog v1: data-chart, code-snippet, lower-third"),
        ),
        ("duration_seconds", json!(15)),
        ("resolution", json!("1080p")),
        ("aspect_ratio", json!("16:9")),
        ("width", json!(1920)),
        ("height", json!(1080)),
        ("fps", json!(30)),
        (
            "plan_snapshot",
            json!({"revision": "video-plan-r1", "scenes": []}),
        ),
        (
            "source_snapshot",
            json!({"revision": "video-source-r1", "files": {}}),
        ),
        (
            "source_bundle",
            json!("SOURCE: notes.md\nGrounded evidence"),
        ),
        ("validation_error", json!("missing title")),
        ("diagram_error", json!("Parse error on line 4")),
        ("headings", json!(["Periodic signals", "Spectrum"])),
        ("retry_present", json!(false)),
        ("retry_error", Value::Null),
        ("retry_invalid_response", Value::Null),
        ("deck_snapshot", json!({"revision": "r1", "slides": {}})),
        (
            "draft_markdown",
            json!("---\nmarp: true\n---\n# Fourier Series\n\n---\n\n## Spectrum"),
        ),
        (
            "issue_report",
            json!({"slide": 2, "vertical_overflow_px": 80}),
        ),
        ("slide_markdown", json!("## Dense\n\nContent")),
        ("max_tool_rounds", json!(8)),
        ("page_size", json!("a4")),
        ("scene_id", json!("scene-2")),
        ("scene_class_name", json!("Scene_scene_2")),
        ("scene_module", json!("scenes/scene_2.py")),
        ("scene_position", json!(2)),
        ("scene_count", json!(5)),
        (
            "scene_snapshot",
            json!({"id": "scene-2", "content": "total internal reflection"}),
        ),
        ("scene_start_seconds", json!(4.0)),
        ("scene_duration_seconds", json!(9.0)),
        (
            "scene_catalog_items",
            json!([{
                "id": "data-chart",
                "source": "<div id=\"data-chart\" data-composition-id=\"data-chart\"></div>"
            }]),
        ),
        ("scene_artifacts", json!(["assets/images/fibre.png"])),
        (
            "scene_narration",
            json!("Light bends back into the core instead of escaping."),
        ),
        (
            "previous_scene_exit",
            json!("the core circle slides left and out of frame"),
        ),
        (
            "frame_measurements",
            json!(
                "- 0.00s: 24.1% of the frame differs from its dominant colour, 33 distinct colours"
            ),
        ),
        ("table_of_contents", json!(true)),
        (
            "document_snapshot",
            json!({"revision": "document-r1", "sections": {}}),
        ),
        (
            "issue",
            json!({
                "page": 2,
                "section": 1,
                "heading": "Primero",
                "kind": "overflows_text_column",
                "overflow_px": 34,
                "element": "table"
            }),
        ),
        ("section_markdown", json!("## Primero\n\nCuerpo.")),
        ("template_enabled", json!(false)),
        ("template_name", json!("")),
        ("template_source", json!("")),
        (
            "reusable_assets",
            json!([{
                "name": "logo",
                "description": "University logo",
                "alt_text": "University emblem",
                "tags": ["branding"],
                "theme": "*",
                "media_type": "image/png",
                "reference": "images/logo.png",
                "content_hash": "abc123"
            }]),
        ),
        (
            "plugins",
            json!([{
                "id": "threejs",
                "name": "Three.js",
                "version": "0.184.0",
                "api_global": "window.SfumatoPlugins.threejs",
                "guidance": "Use a responsive canvas."
            }]),
        ),
        (
            "page_snapshot",
            json!({
                "schema_version": 1,
                "revision": "page-r1",
                "title": "Fourier Explorer",
                "body_html": "<section>Fourier</section>",
                "css": "section { display: grid; }",
                "javascript": ""
            }),
        ),
        (
            "draft_response",
            json!(
                "{\"title\":\"Fourier\",\"body_html\":\"<section>\",\"css\":\"\",\"javascript\":\"\"}"
            ),
        ),
        (
            "requested_prompt",
            json!("A visual comparison of even and odd functions"),
        ),
        ("artifact_name", json!("spectrum")),
        (
            "artifact_description",
            json!("Square-wave harmonic spectrum"),
        ),
        ("artifact_alt_text", json!("Odd harmonic amplitude bars")),
        ("artifact_tags", json!(["fourier", "spectrum"])),
        (
            "generation_recipe",
            json!("Draw odd harmonic bars with decreasing amplitude"),
        ),
    ] {
        values.insert(key.to_string(), value);
    }
    PromptVariables(values)
}

#[test]
fn renders_every_bundled_prompt_with_strict_fixture_values() {
    let catalog = LayeredPromptCatalog::new(None, None);

    for id in PromptId::all() {
        let rendered = catalog
            .render(PromptRenderRequest {
                id: *id,
                variables: representative_variables(),
            })
            .unwrap_or_else(|error| panic!("could not render {id}: {error}"));

        assert!(!rendered.text.is_empty());
        assert_eq!(rendered.provenance.id, *id);
        assert_eq!(rendered.provenance.version, 1);
        assert_eq!(rendered.provenance.content_hash.len(), 64);
        assert_eq!(rendered.provenance.origin, PromptOrigin::Bundled);
    }
}

#[test]
fn page_validation_repair_restates_fragment_boundaries() {
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::PageValidationRepairSystem,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(rendered.text.contains("semantic fragment"));
    for forbidden in ["<html>", "<head>", "<body>", "<style>", "<script>"] {
        assert!(rendered.text.contains(forbidden));
    }
    assert!(rendered.text.contains("Put CSS only in `css`"));
    assert!(rendered.text.contains("JavaScript only in `javascript`"));
    assert!(rendered.text.contains("offline renderer"));
    assert!(rendered.text.contains("\\(...\\)"));
    assert!(rendered.text.contains("\\[...\\]"));
}

#[test]
fn draft_prompts_require_content_only_output_when_a_template_is_selected() {
    let catalog = LayeredPromptCatalog::new(None, None);
    let mut variables = representative_variables();
    variables.0.insert("template_enabled".into(), json!(true));
    variables.0.insert("template_name".into(), json!("lecture"));
    variables.0.insert(
        "template_source".into(),
        json!("---\nmarp: true\n---\n<!-- SFUMATO_CONTENT -->"),
    );

    let slides = catalog
        .render(PromptRenderRequest {
            id: PromptId::SlidesDraftUser,
            variables: variables.clone(),
        })
        .unwrap();
    assert!(slides.text.contains("Return only the Markdown content"));
    assert!(slides.text.contains("Sfumato performs the merge"));

    let page = catalog
        .render(PromptRenderRequest {
            id: PromptId::PageDraftUser,
            variables,
        })
        .unwrap();
    assert!(
        page.text
            .contains("Return only the content that belongs in the marker")
    );
}

#[test]
fn every_slide_markdown_prompt_requires_marp_dollar_math_delimiters() {
    let catalog = LayeredPromptCatalog::new(None, None);
    for id in [
        PromptId::SlidesDraftUser,
        PromptId::SlidesCompactDraftUser,
        PromptId::SlidesValidationRepairUser,
        PromptId::SlidesReviewUser,
        PromptId::SlidesCompactReviewUser,
        PromptId::SlidesLayoutRepairUser,
        PromptId::SlidesCompactLayoutRepairUser,
        PromptId::SlidesEditUser,
        PromptId::SlidesCompactEditUser,
    ] {
        let rendered = catalog
            .render(PromptRenderRequest {
                id,
                variables: representative_variables(),
            })
            .unwrap_or_else(|error| panic!("could not render {id}: {error}"));

        assert!(rendered.text.contains("Use only dollar-sign delimiters"));
        assert!(rendered.text.contains("`$...$`"));
        assert!(rendered.text.contains("`$$...$$`"));
        assert!(rendered.text.contains("Never use LaTeX-style `\\(...\\)`"));
    }
}

#[test]
fn every_prompt_that_can_touch_a_catalog_selection_sees_the_catalog() {
    // The planner, the reviewer's patch and the repair patch can each decide a
    // selection, so all three need the installed item list. A stage that judges
    // selections blind either invents an ID or has its whole patch rejected.
    let catalog = LayeredPromptCatalog::new(None, None);

    for id in [
        PromptId::VideoPlanUser,
        PromptId::VideoReviewUser,
        PromptId::VideoSourceRepairUser,
    ] {
        let rendered = catalog
            .render(PromptRenderRequest {
                id,
                variables: representative_variables(),
            })
            .unwrap_or_else(|error| panic!("could not render {id}: {error}"));

        assert!(
            rendered.text.contains("managed catalog v1: data-chart"),
            "{id} never shows the installed catalog"
        );
    }
}

#[test]
fn every_document_prompt_forbids_authoring_the_page_furniture() {
    // Sfumato composes the cover, the contents and the page numbers from the
    // document's structure. Without this instruction the model writes its own
    // title page and contents list, and the finished PDF carries both.
    let catalog = LayeredPromptCatalog::new(None, None);

    for id in [
        PromptId::DocumentDraftUser,
        PromptId::DocumentCompactDraftUser,
        PromptId::DocumentValidationRepairUser,
        PromptId::DocumentReviewUser,
        PromptId::DocumentCompactReviewUser,
    ] {
        let rendered = catalog
            .render(PromptRenderRequest {
                id,
                variables: representative_variables(),
            })
            .unwrap_or_else(|error| panic!("could not render {id}: {error}"));

        let text = rendered.text.to_ascii_lowercase();
        assert!(
            text.contains("cover") && text.contains("contents"),
            "{id} never tells the model to leave the page furniture alone"
        );
    }
}

#[test]
fn document_prompts_ban_the_math_delimiters_markdown_consumes() {
    // `\(...\)` is an escaped parenthesis in CommonMark, so a document that uses
    // it loses the delimiter before any renderer sees the formula.
    let catalog = LayeredPromptCatalog::new(None, None);

    for id in [
        PromptId::DocumentDraftUser,
        PromptId::DocumentCompactDraftUser,
        PromptId::DocumentValidationRepairUser,
    ] {
        let rendered = catalog
            .render(PromptRenderRequest {
                id,
                variables: representative_variables(),
            })
            .unwrap();

        assert!(
            rendered.text.contains("$inline$") && rendered.text.contains("$$display$$"),
            "{id} must name the dollar delimiters"
        );
    }
}

#[test]
fn the_video_planner_is_told_to_reach_for_generated_images() {
    // The previous wording told the planner to reuse existing artifacts before
    // requesting an image, which is why real videos shipped with none: flat vector
    // shapes where a purpose-built illustration was the point.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoPlanUser,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(rendered.text.contains("sfumato_image_gen"));
    assert!(
        !rendered
            .text
            .contains("Reuse suitable existing artifacts before requesting"),
        "the discouraging wording is gone"
    );
}

#[test]
fn the_video_author_learns_how_to_embed_a_selected_image() {
    // The author never saw a word about images, so a generated illustration could
    // not reach the film even when the planner had produced one.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoHyperframeSceneUser,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(rendered.text.contains("assets/images/"));
    assert!(
        rendered.text.contains("<img src="),
        "it needs the embedding form"
    );
    // Pruning runs against the authored source, so an image the author ignores is
    // deleted. Saying so is what stops a generated illustration from vanishing.
    assert!(rendered.text.contains("deleted before the render"));
}

#[test]
fn the_video_reviewer_can_be_told_its_answer_was_rejected() {
    // Without the corrective retry block the reviewer gets no chance to fix a
    // malformed patch, and one bad answer discards the whole review.
    let catalog = LayeredPromptCatalog::new(None, None);
    let mut variables = representative_variables();
    variables.0.insert("retry_present".into(), json!(true));
    variables
        .0
        .insert("retry_error".into(), json!("must be an RFC 6902 patch"));
    variables.0.insert(
        "retry_invalid_response".into(),
        json!("sure, here are my notes"),
    );

    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoReviewUser,
            variables,
        })
        .unwrap();

    assert!(rendered.text.contains("Corrective retry"));
    assert!(rendered.text.contains("must be an RFC 6902 patch"));
}

#[test]
fn the_video_author_is_told_not_to_open_a_scene_on_nothing() {
    // Measured on a real film: all four scene boundaries rendered empty, because
    // nothing told the author that elements must already be on screen when a scene
    // starts. The deterministic gate rejects it; this is what prevents it.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoHyperframeSceneSystem,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(
        rendered
            .text
            .contains("Never open a scene on an empty frame")
    );
    assert!(
        rendered.text.contains("perform, not breathe"),
        "idle motion is banned"
    );
    // The doctrine has to live in the prompt that actually authors scenes. It once
    // sat in the whole-film prompt, which per-scene authoring had already retired.
    assert!(
        rendered.text.contains("one scene of a Hyperframes film"),
        "the rules belong to the live authoring path"
    );
}

#[test]
fn the_video_planner_must_choose_a_visual_means_per_beat() {
    // A real plan used one catalog item across five scenes and drew the rest by
    // hand, which is why the film looked like a diagram.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoPlanUser,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(rendered.text.contains("pick exactly one of"));
    assert!(rendered.text.contains("generated illustration"));
}

#[test]
fn the_scene_author_receives_the_previous_beat_exit_and_its_own_window() {
    // The seam rule only becomes actionable when the author knows what it is
    // entering from, and the empty-frame rule has to be restated where the author
    // is actually writing the first frame.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoHyperframeSceneUser,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(
        rendered
            .text
            .contains("the core circle slides left and out of frame")
    );
    assert!(
        rendered
            .text
            .contains("already be visible at the scene's first frame")
    );
    // A scene's own timeline is scene-relative; treating it as film-relative is a
    // classic way to author a scene that plays at the wrong moment.
    assert!(
        rendered
            .text
            .contains("start at 0 even though the film places it at 4")
    );
    assert!(rendered.text.contains("Reference: `data-chart`"));
    assert!(rendered.text.contains("assets/images/fibre.png"));
}

#[test]
fn bundled_prompt_rendering_matches_the_reviewed_aggregate_snapshot() {
    let catalog = LayeredPromptCatalog::new(None, None);
    let mut aggregate = String::new();
    for id in PromptId::all() {
        let rendered = catalog
            .render(PromptRenderRequest {
                id: *id,
                variables: representative_variables(),
            })
            .unwrap();
        aggregate.push_str(id.as_str());
        aggregate.push('\n');
        aggregate.push_str(&rendered.text);
        aggregate.push_str("\n\0\n");
    }

    assert_eq!(
        format!("{:x}", Sha256::digest(aggregate.as_bytes())),
        "eacac02db2ec67f5484db2a97302daf53ead160579d35752bbafbdf207d0c42a"
    );
}

#[test]
fn project_override_wins_and_invalid_override_never_falls_back() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let user = temp.path().join("user-prompts");
    let catalog = LayeredPromptCatalog::new(Some(project.clone()), Some(user.clone()));
    let id = PromptId::SlidesDraftSystem;
    let relative = catalog
        .list()
        .unwrap()
        .into_iter()
        .find(|template| template.id == id)
        .unwrap()
        .path;

    let user_path = user.join(&relative);
    fs::create_dir_all(user_path.parent().unwrap()).unwrap();
    fs::write(&user_path, "User {{ learning_style | join(', ') }}").unwrap();
    let project_path = project.join(".sfumato/prompts").join(&relative);
    fs::create_dir_all(project_path.parent().unwrap()).unwrap();
    fs::write(&project_path, "Project {{ learning_style | join(', ') }}").unwrap();

    let rendered = catalog
        .render(PromptRenderRequest {
            id,
            variables: representative_variables(),
        })
        .unwrap();
    assert!(rendered.text.starts_with("Project"));
    assert_eq!(
        rendered.provenance.origin,
        PromptOrigin::Project(project_path.clone())
    );

    fs::write(&project_path, "Broken {{ missing_value }}").unwrap();
    let error = catalog
        .render(PromptRenderRequest {
            id,
            variables: representative_variables(),
        })
        .unwrap_err();
    assert!(error.to_string().contains("undefined value"));
}

#[test]
fn customization_copies_bundled_source_without_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let catalog =
        LayeredPromptCatalog::new(Some(project), Some(PathBuf::from(temp.path()).join("user")));

    let path = catalog
        .customize(PromptId::SlidesEditUser, PromptOverrideScope::User)
        .unwrap();

    assert!(path.is_file());
    assert!(
        catalog
            .customize(PromptId::SlidesEditUser, PromptOverrideScope::User)
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
}

#[test]
fn oversized_and_escaping_overrides_fail_without_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let catalog = LayeredPromptCatalog::new(Some(project.clone()), None);
    let relative = catalog
        .list()
        .unwrap()
        .into_iter()
        .find(|template| template.id == PromptId::SlidesDraftSystem)
        .unwrap()
        .path;
    let path = project.join(".sfumato/prompts").join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "x".repeat(65 * 1024)).unwrap();

    let error = catalog.validate().unwrap_err();
    assert!(error.to_string().contains("byte limit"));

    fs::write(&path, "{% include \"../outside.md.j2\" %}").unwrap();
    let error = catalog
        .render(PromptRenderRequest {
            id: PromptId::SlidesDraftSystem,
            variables: representative_variables(),
        })
        .unwrap_err();
    assert!(error.to_string().contains("outside.md.j2"));
}

#[cfg(unix)]
#[test]
fn symlinked_override_is_rejected() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let override_root = project.join(".sfumato/prompts/slides");
    fs::create_dir_all(&override_root).unwrap();
    let outside = temp.path().join("outside.j2");
    fs::write(&outside, "outside").unwrap();
    symlink(&outside, override_root.join("draft.system.md.j2")).unwrap();
    let catalog = LayeredPromptCatalog::new(Some(project), None);

    assert!(
        catalog
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unsafe")
    );
}

#[test]
fn the_visual_reviewer_judges_pixels_and_not_intentions() {
    let catalog = LayeredPromptCatalog::new(None, None);

    let system = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoVisualReviewSystem,
            variables: representative_variables(),
        })
        .unwrap();

    // Reporting a defect the plan implies rather than the frame shows is how a
    // visual review turns into a second semantic review that repairs nothing.
    assert!(system.text.contains("Report only what the frames show"));
    assert!(system.text.contains("legible"));
    assert!(system.text.contains("overlap"));
    // Stillness is craft: the deterministic gate already handles empty frames, and
    // a reviewer that flags every held beat makes the whole layer noise.
    assert!(system.text.contains("Deliberate stillness"));
    assert!(system.text.contains("\"approved\""));
    assert!(system.text.contains("\"findings\""));

    let user = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoVisualReviewUser,
            variables: representative_variables(),
        })
        .unwrap();

    // The measurements travel with the frames so the model spends its attention on
    // what counting pixels cannot see.
    assert!(user.text.contains("Sfumato already measured"));
    assert!(user.text.contains("Judge what the measurements cannot"));
    assert!(user.text.contains("labelled with its timeline position"));
}

#[test]
fn the_scene_author_adapts_a_catalog_technique_rather_than_mounting_a_showcase() {
    // Blocks and components stage into different directories. The prompt used to
    // derive one path from the item ID, so a real film mounted
    // `compositions/morph-text.html` for a component that lives under
    // `compositions/components/`, and the renderer reported a missing
    // sub-composition after every scene had already been authored.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoHyperframeSceneUser,
            variables: representative_variables(),
        })
        .unwrap();

    // The item's own markup, so the author can rebuild the technique.
    assert!(rendered.text.contains("data-composition-id=\"data-chart\""));
    // These files are showcase documents. Mounting one put "Should I learn to code?"
    // into a film about fibre optics, and the author then hid it under its own
    // ground until the renderer rejected the scene for text it could not see.
    assert!(
        rendered
            .text
            .contains("Never mount one and never copy its copy")
    );
    assert!(rendered.text.contains("showcase document, not a component"));
}

#[test]
fn the_scene_author_is_told_the_legibility_faults_that_fail_a_render() {
    // Both are hard renderer errors, not lint advice: a real film failed on a 4.29:1
    // text run and on two labels that overlapped mid-transition.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoHyperframeSceneSystem,
            variables: representative_variables(),
        })
        .unwrap();

    // A track holds one clip at a time; a real film failed with two clips on track 3.
    assert!(rendered.text.contains("A track holds one clip at a time"));
    // Each of the three overflow remedies is tied to a fault measured on a real
    // film: a fixed-height box broken by one capital letter, a hand-picked width
    // clipping a formula by 80px, and a word-by-word reveal left on top of the
    // unsplit phrase.
    for remedy in [
        "Never fix the height of a box that holds text",
        "line-height` of at least 1.15",
        "Never pick a text box's width by eye",
        "delete the unsplit copy",
    ] {
        assert!(
            rendered.text.contains(remedy),
            "missing the {remedy} rule: {}",
            rendered.text
        );
    }
    for fault in [
        "4.5:1",
        "opaque element over text",
        "share space",
        "inside its own box",
    ] {
        assert!(
            rendered.text.contains(fault),
            "missing the {fault} rule: {}",
            rendered.text
        );
    }
    assert!(
        rendered.text.contains("including mid-transition"),
        "the overlap has to be checked while things move"
    );
}

#[test]
fn the_video_reviewer_is_told_which_root_its_paths_address() {
    // The snapshot wraps the plan, so the reviewer wrote `/document/scenes/...` and
    // every patch was rejected. A real film shipped with its review discarded for
    // exactly this, twice, including the corrective retry.
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoReviewSystem,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(rendered.text.contains("never `/document/scenes/0/content`"));
    assert!(
        rendered
            .text
            .contains("the `/revision` test, which addresses the snapshot")
    );
}

#[test]
fn the_scene_author_is_shown_the_faults_and_its_own_previous_attempt() {
    // The repair collected the renderer's measurements, put them in the context and
    // rendered a template that referenced neither. Every repair was a blind re-roll:
    // the author never learned what failed, and could not make a minimal edit
    // because it could not see what it had written.
    let catalog = LayeredPromptCatalog::new(None, None);
    let mut variables = representative_variables();
    variables.0.insert("retry_present".into(), json!(true));
    variables.0.insert(
        "retry_error".into(),
        json!("clipped_text #scene-3-ratio overflowed right 80px"),
    );
    variables.0.insert(
        "retry_invalid_response".into(),
        json!("<template>…</template>"),
    );

    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoHyperframeSceneUser,
            variables,
        })
        .unwrap();

    // The measurement itself, which is the number the author needs to recover.
    assert!(
        rendered.text.contains("overflowed right 80px"),
        "{}",
        rendered.text
    );
    assert!(rendered.text.contains("<template>…</template>"));
    // Minimal edit, because a redesign spends the attempt and adds new faults.
    assert!(
        rendered
            .text
            .contains("Fix **every** fault listed, and change nothing else")
    );
    assert!(rendered.text.contains("margin beyond it"));
}

#[test]
fn a_first_attempt_carries_no_corrective_block() {
    let catalog = LayeredPromptCatalog::new(None, None);
    let rendered = catalog
        .render(PromptRenderRequest {
            id: PromptId::VideoHyperframeSceneUser,
            variables: representative_variables(),
        })
        .unwrap();

    assert!(!rendered.text.contains("Corrective retry"));
}

#[test]
fn validate_reports_an_override_placed_at_the_wrong_path() {
    // Someone edits a prompt by hand, generation is unchanged, and the command
    // whose job is to check prompts used to answer "Validated 69 prompt
    // templates." with nothing else.
    let user = tempfile::tempdir().expect("user override root");
    fs::create_dir_all(user.path().join("videos")).unwrap();
    fs::write(user.path().join("videos/plan.system.md"), "{{ roto").unwrap();
    let catalog = LayeredPromptCatalog::new(None, Some(user.path().to_path_buf()));

    let validation = catalog
        .validate()
        .expect("bundled templates still validate");

    assert_eq!(validation.unreferenced.len(), 1);
    let stray = &validation.unreferenced[0];
    assert!(stray.path.ends_with("videos/plan.system.md"));
    assert_eq!(
        stray.expected.as_deref(),
        Some(std::path::Path::new("video/plan.system.md.j2"))
    );
}

#[test]
fn validate_reports_a_j2_override_the_manifest_does_not_list() {
    // The quieter half: a `.j2` at an unlisted path compiles into the
    // environment and is then never asked for.
    let user = tempfile::tempdir().expect("user override root");
    fs::create_dir_all(user.path().join("videos")).unwrap();
    fs::write(user.path().join("videos/plan.system.md.j2"), "hello").unwrap();
    let catalog = LayeredPromptCatalog::new(None, Some(user.path().to_path_buf()));

    let validation = catalog
        .validate()
        .expect("bundled templates still validate");

    assert_eq!(validation.unreferenced.len(), 1);
    assert_eq!(
        validation.unreferenced[0].expected.as_deref(),
        Some(std::path::Path::new("video/plan.system.md.j2"))
    );
}

#[test]
fn validate_stays_quiet_for_an_override_at_the_path_customize_writes() {
    let user = tempfile::tempdir().expect("user override root");
    let catalog = LayeredPromptCatalog::new(None, Some(user.path().to_path_buf()));
    let written = catalog
        .customize(PromptId::VideoPlanSystem, PromptOverrideScope::User)
        .expect("customize writes an override");

    let validation = catalog.validate().expect("the override validates");

    assert!(
        validation.unreferenced.is_empty(),
        "a correct override must not warn: {:?}",
        validation.unreferenced
    );
    assert!(written.ends_with("video/plan.system.md.j2"));
}

#[test]
fn an_unrecognisable_stray_is_reported_without_a_guess() {
    // A suggestion that is wrong is worse than none, so only an unambiguous
    // name match offers one.
    let user = tempfile::tempdir().expect("user override root");
    fs::write(user.path().join("notes.txt"), "scratch").unwrap();
    let catalog = LayeredPromptCatalog::new(None, Some(user.path().to_path_buf()));

    let validation = catalog
        .validate()
        .expect("bundled templates still validate");

    assert_eq!(validation.unreferenced.len(), 1);
    assert_eq!(validation.unreferenced[0].expected, None);
}
