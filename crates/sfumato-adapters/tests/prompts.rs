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
        "0030d250cf90e8dd716a469b0afea8c326340af8fc4c57347c27ebb5b03fe5a7"
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
