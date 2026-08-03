use std::collections::BTreeMap;

use crate::{
    ReviewableDocument, VideoEngine, VideoPlanDocument, VideoScene, VideoSceneProduction,
    VideoSourceDocument, VideoWorkflow,
};

#[test]
fn video_plan_is_revision_guarded_and_reviewable() {
    let mut plan = VideoPlanDocument::new(
        VideoEngine::Model,
        "Fourier",
        "Explain the frequency domain",
        8,
        vec![VideoScene {
            id: "intro".into(),
            start_seconds: 0.0,
            duration_seconds: 8.0,
            content: "Introduce harmonics".into(),
            visual: "Animate a square wave".into(),
            artifacts: Vec::new(),
            narration: String::new(),
            production: VideoSceneProduction::default(),
        }],
        Vec::new(),
        "Use strong contrast",
        "Animate a square wave decomposing into harmonics",
    )
    .unwrap();
    let patch = serde_json::from_value(serde_json::json!([
        {"op":"test","path":"/revision","value":plan.revision()},
        {"op":"replace","path":"/objective","value":"Explain harmonic decomposition"}
    ]))
    .unwrap();
    let report = plan.apply_patch(&patch).unwrap();
    assert_eq!(report.changed_nodes, vec!["objective"]);
}

#[test]
fn hyperframe_plans_require_pipeline_direction_and_limit_motion_rules() {
    let mut plan = VideoPlanDocument::new(
        VideoEngine::Hyperframe,
        "Fourier",
        "Explain the frequency domain",
        8,
        vec![VideoScene {
            id: "intro".into(),
            start_seconds: 0.0,
            duration_seconds: 8.0,
            content: "Introduce harmonics".into(),
            visual: "Animate a square wave".into(),
            artifacts: Vec::new(),
            narration: String::new(),
            production: VideoSceneProduction::default(),
        }],
        Vec::new(),
        "Use strong contrast",
        "",
    )
    .unwrap();
    plan.set_pipeline(
        VideoWorkflow::Explainer,
        "One message",
        "hook to payoff",
        "dark editorial",
    )
    .unwrap();
    assert!(plan.validate().is_ok());
}

#[test]
fn structured_scene_direction_is_normalized_without_losing_information() {
    let scene: VideoScene = serde_json::from_value(serde_json::json!({
        "id": "inside-cable",
        "start_seconds": 0,
        "duration_seconds": 4,
        "content": {"concept": "total internal reflection"},
        "visual": {"camera": "cross section", "focus": "core"},
        "artifacts": [],
        "production": {
            "narrative_role": "explanation",
            "on_screen_copy": [{"primary": "Luz guiada"}],
            "focal_element": {"type": "fiber-core"},
            "layout": {"center": "cross-section", "right": "labels"},
            "layers": [{"background": "dark"}, {"foreground": "labels"}],
            "motion_rules": ["trace-light"],
            "entrance": "fade",
            "exit": "wipe",
            "transition": {"type": "cover", "direction": "left"},
            "acceptance": [{"visible": "core and cladding"}]
        }
    }))
    .unwrap();

    assert!(scene.visual.contains("\"camera\":\"cross section\""));
    assert!(
        scene
            .production
            .layout
            .contains("\"center\":\"cross-section\"")
    );
    assert_eq!(scene.production.layers.len(), 2);
    assert!(scene.production.transition.contains("\"type\":\"cover\""));
}

/// Builds a one-scene plan whose only variable is the scene identifier.
fn plan_with_scene_id(id: &str) -> Result<VideoPlanDocument, crate::ReviewError> {
    VideoPlanDocument::new(
        VideoEngine::Model,
        "Fourier",
        "Explain the frequency domain",
        8,
        vec![VideoScene {
            id: id.into(),
            start_seconds: 0.0,
            duration_seconds: 8.0,
            content: "Introduce harmonics".into(),
            visual: "Animate a square wave".into(),
            artifacts: Vec::new(),
            narration: String::new(),
            production: VideoSceneProduction::default(),
        }],
        Vec::new(),
        "Use strong contrast",
        "Animate a square wave decomposing into harmonics",
    )
}

#[test]
fn accepts_the_scene_identifiers_a_planner_actually_writes() {
    for id in ["intro", "beat-2", "proof_point", "s3", "A"] {
        assert!(
            plan_with_scene_id(id).is_ok(),
            "`{id}` is a reasonable scene name and must stay valid"
        );
    }
}

#[test]
fn rejects_a_scene_identifier_that_escapes_its_directory() {
    // The ID becomes a file name: narration writes `narration-<id>-<digest>.mp3`
    // into the staging directory, so a traversal here writes outside the project.
    for id in [
        "../../../../tmp/pwned",
        "..",
        "nested/scene",
        "scene\\windows",
        "/absolute",
    ] {
        let error = plan_with_scene_id(id).expect_err("`{id}` must be rejected");
        assert!(
            error.to_string().contains("scene identifier"),
            "`{id}` was rejected for the wrong reason: {error}"
        );
    }
}

#[test]
fn rejects_a_scene_identifier_that_could_break_out_of_an_attribute() {
    // The ID is interpolated into `data-composition-src="…"` in the master
    // composition, which a headless browser then executes.
    for id in [
        "a\" onload=\"fetch('http://evil')",
        "a'><script>alert(1)</script>",
        "a<b",
        "a&b",
        "a b",
    ] {
        assert!(
            plan_with_scene_id(id).is_err(),
            "`{id}` can inject markup and must be rejected"
        );
    }
}

#[test]
fn rejects_empty_padded_and_oversized_scene_identifiers() {
    for id in ["", " ", " intro ", "-intro", "intro-", "_intro"] {
        assert!(
            plan_with_scene_id(id).is_err(),
            "`{id}` is not a usable name component"
        );
    }
    assert!(plan_with_scene_id(&"a".repeat(64)).is_ok());
    assert!(plan_with_scene_id(&"a".repeat(65)).is_err());
}

#[test]
fn a_review_patch_cannot_smuggle_in_an_unsafe_scene_identifier() {
    // Validation on construction is not enough on its own: the reviewer edits the
    // plan through a patch, and that path re-validates the patched document.
    let mut plan = plan_with_scene_id("intro").unwrap();
    let patch = serde_json::from_value(serde_json::json!([
        {"op":"test","path":"/revision","value":plan.revision()},
        {"op":"replace","path":"/scenes/0/id","value":"../../../../tmp/pwned"}
    ]))
    .unwrap();
    assert!(plan.apply_patch(&patch).is_err());
    assert_eq!(
        plan.scenes()[0].id,
        "intro",
        "a rejected patch changes nothing"
    );
}

#[test]
fn source_repair_cannot_add_files() {
    let source = VideoSourceDocument::new(
        VideoEngine::Manim,
        BTreeMap::from([("scene.py".into(), "from manim import *".into())]),
    )
    .unwrap();
    let patch = serde_json::from_value(serde_json::json!([
        {"op":"test","path":"/revision","value":source.snapshot().unwrap().revision},
        {"op":"add","path":"/files/extra.py","value":"print('no')"}
    ]))
    .unwrap();
    assert!(source.validate_patch(&patch).is_err());
}
