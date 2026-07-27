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
