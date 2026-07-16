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
            "issue_report",
            json!({"slide": 2, "vertical_overflow_px": 80}),
        ),
        ("slide_markdown", json!("## Dense\n\nContent")),
        ("max_tool_rounds", json!(8)),
        (
            "requested_prompt",
            json!("A visual comparison of even and odd functions"),
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
        "5f452db2486e876551c8c31462c22cc0223bbdd95b8a04e85ed01e0208581d2e"
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
