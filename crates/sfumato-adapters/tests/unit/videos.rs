use super::*;

#[test]
fn managed_renderer_manifest_pins_supported_packages() {
    let hyperframe = renderer_package("hyperframe").unwrap();
    let manim = renderer_package("manim").unwrap();

    assert_eq!(hyperframe.package, "hyperframes");
    assert_eq!(hyperframe.version, "0.7.62");
    assert_eq!(
        hyperframe.runtime_packages.get("gsap").map(String::as_str),
        Some("3.15.0")
    );
    assert_eq!(manim.package, "manim");
    assert_eq!(manim.version, "0.20.1");
    assert!(renderer_package("unknown").is_err());
}

#[test]
fn hyperframes_optional_capabilities_do_not_make_the_renderer_unhealthy() {
    let report: HyperframesDoctorReport = serde_json::from_value(serde_json::json!({
        "ok": false,
        "checks": [
            { "name": "Node.js", "ok": true, "detail": "v22" },
            { "name": "FFmpeg", "ok": true, "detail": "installed" },
            { "name": "FFprobe", "ok": true, "detail": "installed" },
            { "name": "Chrome", "ok": true, "detail": "installed" },
            {
                "name": "TTS (Kokoro)",
                "ok": false,
                "detail": "Not installed",
                "hint": "pip install kokoro-onnx"
            },
            { "name": "Docker running", "ok": false, "detail": "Not running" }
        ]
    }))
    .unwrap();

    let (healthy, details) = evaluate_hyperframes_doctor(report);

    assert!(healthy);
    assert_eq!(details.len(), 1);
    assert_eq!(
        details[0],
        "optional capabilities unavailable: TTS (Kokoro), Docker running"
    );
}

#[test]
fn hyperframes_missing_required_dependency_is_unhealthy() {
    let report: HyperframesDoctorReport = serde_json::from_value(serde_json::json!({
        "checks": [
            { "name": "Node.js", "ok": true, "detail": "v22" },
            { "name": "FFmpeg", "ok": true, "detail": "installed" },
            { "name": "FFprobe", "ok": true, "detail": "installed" },
            { "name": "Chrome", "ok": false, "detail": "Not found" }
        ]
    }))
    .unwrap();

    let (healthy, details) = evaluate_hyperframes_doctor(report);

    assert!(!healthy);
    assert_eq!(details, vec!["required Chrome unavailable: Not found"]);
}
