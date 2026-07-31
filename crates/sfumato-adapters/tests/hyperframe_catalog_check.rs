//! Checks that a minimal project passes the renderer with the catalog staged.
//!
//! Run explicitly: `cargo test -p sfumato-adapters --test hyperframe_catalog_check -- --ignored`

use std::path::PathBuf;

use sfumato_adapters::videos::ManagedVideoRenderers;
use sfumato_core::{
    operation::OperationContext,
    renderers::{VideoEngine, VideoRenderRequest, VideoRenderer},
};

#[tokio::test]
#[ignore = "drives the managed Hyperframes renderer"]
async fn a_minimal_project_passes_check_with_every_catalog_item_staged() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let root = workspace.path().to_path_buf();
    std::fs::create_dir_all(root.join("compositions")).unwrap();
    std::fs::write(root.join("meta.json"), "{\n  \"name\": \"probe\"\n}\n").unwrap();
    std::fs::write(
        root.join("compositions/scene-1.html"),
        "<template>\n<div id=\"scene-1\" data-composition-id=\"scene-1\" data-width=\"1280\" data-height=\"720\" data-start=\"0\">\n<h1 id=\"t\" class=\"clip\" data-start=\"0\" data-duration=\"4\" data-track-index=\"0\">Luz</h1>\n</div>\n<script>\nconst tl = gsap.timeline({ paused: true });\ntl.to(\"#t\", { x: 40, duration: 1 }, 0);\ntl.set({}, {}, 4);\nwindow.__timelines = window.__timelines || {};\nwindow.__timelines[\"scene-1\"] = tl;\n</script>\n</template>\n",
    )
    .unwrap();
    std::fs::write(
        root.join("index.html"),
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"UTF-8\"></head>\n<body>\n  <div id=\"root\" data-composition-id=\"root\" data-start=\"0\" data-width=\"1280\" data-height=\"720\">\n    <div id=\"mount-scene-1\" class=\"clip\" data-composition-id=\"mount-scene-1\" data-composition-src=\"compositions/scene-1.html\" data-start=\"0\" data-duration=\"4\" data-track-index=\"0\"></div>\n    <script src=\"./vendor/gsap.min.js\"></script>\n    <script>\n      const tl = gsap.timeline({ paused: true });\n      tl.set({}, {}, 4);\n      window.__timelines = window.__timelines || {};\n      window.__timelines[\"root\"] = tl;\n    </script>\n  </div>\n</body>\n</html>\n",
    )
    .unwrap();

    let renderers = ManagedVideoRenderers::default_path().expect("managed root");
    let request = VideoRenderRequest {
        source_root: root.clone(),
        output_path: root.join("out.mp4"),
        duration_seconds: 4,
        width: 1280,
        height: 720,
        fps: 30,
        quality: "draft".into(),
    };

    let result = renderers
        .validate(VideoEngine::Hyperframe, &request, &OperationContext::detached())
        .await;

    if let Err(error) = &result {
        println!("CHECK FAILED:\n{}", error.message);
    }
    let staged: Vec<PathBuf> = std::fs::read_dir(root.join("compositions"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    println!("staged compositions: {}", staged.len());
    assert!(result.is_ok(), "a minimal project must pass with the catalog staged");
}
