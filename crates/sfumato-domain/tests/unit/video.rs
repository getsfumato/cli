use std::collections::BTreeMap;

use crate::{ReviewableDocument, VideoEngine, VideoPlanDocument, VideoScene, VideoSourceDocument};

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
