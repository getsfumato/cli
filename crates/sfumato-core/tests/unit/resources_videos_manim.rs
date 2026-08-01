use super::*;

use crate::resources::{narration::CaptionGroup, videos::assembly::NarrationClip};
use sfumato_domain::{VideoEngine, VideoWorkflow};

/// Two scenes starting at 0 and 4 seconds, the shape of a real film.
fn plan() -> VideoPlanDocument {
    let response = format!(
        r#"{{"title":"Laplace","objective":"teach","workflow":"explainer","message":"m","narrative_arc":"hook","design_direction":"d","scenes":[{},{}],"artifacts":[],"visual_direction":"vd","remote_prompt":""}}"#,
        r#"{"id":"intro","start_seconds":0,"duration_seconds":4,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"hook"}}"#,
        r#"{"id":"region-of-convergence","start_seconds":4,"duration_seconds":6.5,"content":"c","visual":"v","artifacts":[],"production":{"narrative_role":"body"}}"#,
    );
    super::super::parse_plan(
        &response,
        VideoEngine::Manim,
        24,
        Some("Laplace"),
        VideoWorkflow::Explainer,
        None,
    )
    .expect("plan parses")
    .0
}

#[test]
fn a_scene_id_becomes_a_legal_module_and_class() {
    assert_eq!(scene_symbol("intro-beat"), "intro_beat");
    assert_eq!(scene_module_path("intro-beat"), "scenes/intro_beat.py");
    assert_eq!(scene_class_name("intro-beat"), "Scene_intro_beat");
    // A plan is free to name a scene "01", but a Python module is not.
    assert_eq!(scene_module_path("01-opening"), "scenes/s_01_opening.py");
    assert_eq!(scene_class_name("01-opening"), "Scene_s_01_opening");
}

#[test]
fn the_manifest_carries_the_timeline_the_model_never_wrote() {
    let plan = plan();
    let layer = NarrationLayer {
        clips: vec![
            NarrationClip {
                reference: "assets/audio/a.mp3".to_string(),
                start_seconds: 0.0,
                duration_seconds: 3.6,
            },
            NarrationClip {
                reference: "assets/audio/b.mp3".to_string(),
                start_seconds: 4.0,
                duration_seconds: 6.1,
            },
        ],
        captions: true,
    };
    let manifest = manim_manifest(&plan, &layer);

    assert_eq!(manifest.scenes.len(), 2);
    assert_eq!(manifest.scenes[0].id, "intro");
    assert_eq!(manifest.scenes[0].module, "scenes/intro.py");
    assert_eq!(manifest.scenes[0].class_name, "Scene_intro");
    assert_eq!(manifest.scenes[1].duration_seconds, 6.5);
    // The offsets are the spoken ones, so the mix lands where the voice was timed.
    assert_eq!(manifest.audio[1].start_seconds, 4.0);
    let captions = manifest.captions.as_ref().expect("captions were generated");
    assert_eq!(captions.module, "captions.py");
    assert_eq!(captions.class_name, "SfumatoCaptions");
}

#[test]
fn a_film_without_narration_declares_no_audio_and_no_captions() {
    let manifest = manim_manifest(&plan(), &NarrationLayer::default());
    assert!(manifest.audio.is_empty());
    assert!(manifest.captions.is_none());
}

#[test]
fn the_caption_overlay_is_a_generated_manim_scene() {
    let groups = vec![
        CaptionGroup {
            text: "La transformada de Laplace".to_string(),
            start_seconds: 0.5,
            end_seconds: 2.25,
            words: Vec::new(),
        },
        CaptionGroup {
            text: "convierte una ecuación diferencial".to_string(),
            start_seconds: 2.25,
            end_seconds: 4.0,
            words: Vec::new(),
        },
    ];
    let module = captions_module(&groups, 10.5, 1080);

    assert!(module.contains("class SfumatoCaptions(Scene):"));
    assert!(module.contains("La transformada de Laplace"));
    assert!(module.contains("2.25"));
    // Held to the film's full length so the overlay and the picture end together.
    assert!(module.contains("TOTAL = 10.5"));
    // The overlay is composited, so it must never paint its own background.
    assert!(!module.contains("background_color"));
}

#[test]
fn caption_text_cannot_break_out_of_the_module_it_is_embedded_in() {
    let groups = vec![CaptionGroup {
        // A quote and a backslash in narration are ordinary; unescaped they would
        // end the Python string literal and produce a module that will not compile.
        text: "she said \"hola\" \\ then left".to_string(),
        start_seconds: 0.0,
        end_seconds: 1.0,
        words: Vec::new(),
    }];
    let module = captions_module(&groups, 2.0, 1080);

    assert!(module.contains(r#"\"hola\""#));
    assert!(module.contains(r"\\"));
    // The literal must still be one well-formed JSON array, which is also a valid
    // Python list of strings and numbers.
    let literal = module
        .lines()
        .find_map(|line| line.strip_prefix("GROUPS = "))
        .expect("the module declares its groups");
    let parsed: serde_json::Value =
        serde_json::from_str(literal).expect("the embedded literal is well formed");
    assert_eq!(parsed[0][0], "she said \"hola\" \\ then left");
}

#[test]
fn caption_type_scales_with_the_canvas() {
    let groups = vec![CaptionGroup {
        text: "x".to_string(),
        start_seconds: 0.0,
        end_seconds: 1.0,
        words: Vec::new(),
    }];
    // A vertical cut and a widescreen one must read the same, which a fixed point
    // size cannot do across a 1.8x difference in height.
    assert!(captions_module(&groups, 2.0, 1080).contains("FONT_SIZE = 45"));
    assert!(captions_module(&groups, 2.0, 1920).contains("FONT_SIZE = 81"));
}

#[test]
fn the_film_runs_to_the_end_of_its_last_scene() {
    // Summing durations would be wrong for a retimed film, where a scene's start
    // is the offset the voice actually produced.
    assert_eq!(film_seconds(&plan()), 10.5);
}

const VALID_SCENE: &str = "from manim import *\n\
     import numpy as np\n\n\
     class Scene_intro(Scene):\n\
     \x20   def construct(self):\n\
     \x20       title = MathTex(r\"F(s)\")\n\
     \x20       self.play(Write(title), run_time=2.0)\n\
     \x20       self.wait(2.0)\n";

#[test]
fn a_well_formed_scene_module_is_accepted() {
    validate_scene_module("intro", VALID_SCENE).expect("the fixture should be valid");
}

#[test]
fn a_scene_that_defines_the_wrong_class_is_named_in_the_complaint() {
    let error = validate_scene_module("region-of-convergence", VALID_SCENE)
        .expect_err("the class does not match the scene");
    assert!(error.contains("Scene_region_of_convergence"));
    assert!(error.contains("scenes/region_of_convergence.py"));
}

#[test]
fn a_scene_that_never_animates_or_holds_is_refused() {
    let source = "from manim import *\n\n\
         class Scene_intro(Scene):\n\
         \x20   def construct(self):\n\
         \x20       self.add(Dot())\n";
    // Without a wait the beat renders as a handful of frames and the narration
    // runs on over a frozen picture.
    assert!(validate_scene_module("intro", source).is_err());
}

#[test]
fn a_scene_that_overrides_a_film_wide_decision_is_refused() {
    for statement in [
        "config.frame_rate = 12",
        "self.add_sound(\"a.mp3\")",
        "if __name__ == '__main__':",
    ] {
        let source = format!("{VALID_SCENE}        {statement}\n");
        let error = validate_scene_module("intro", &source)
            .expect_err(&format!("expected {statement:?} to be refused"));
        assert!(!error.is_empty());
    }
}

#[test]
fn a_scene_that_reaches_outside_its_run_directory_is_refused() {
    let source = format!("{VALID_SCENE}        import os\n");
    assert!(validate_scene_module("intro", &source).is_err());
}

#[test]
fn a_fenced_response_is_unwrapped_to_the_module_itself() {
    let fenced = format!("```python\n{VALID_SCENE}```");
    assert_eq!(strip_python_fence(&fenced), VALID_SCENE.trim());
    // An unfenced response is already the module.
    assert_eq!(strip_python_fence(VALID_SCENE), VALID_SCENE.trim());
}

/// Writes a generated caption module so the Manim renderer can be run over it.
///
/// Guarded behind the renderer feature because it exists to be executed by hand
/// against a real Manim install, not to assert anything on its own.
#[cfg(feature = "real-renderers")]
#[test]
fn emit_caption_module_fixture() {
    let groups = vec![
        CaptionGroup {
            text: "La transformada de Laplace".to_string(),
            start_seconds: 0.2,
            end_seconds: 1.8,
            words: Vec::new(),
        },
        CaptionGroup {
            text: "convierte una ecuación \"difícil\"".to_string(),
            start_seconds: 2.4,
            end_seconds: 4.2,
            words: Vec::new(),
        },
    ];
    std::fs::write(
        std::env::temp_dir().join("sfumato-captions-fixture.py"),
        captions_module(&groups, 6.0, 360),
    )
    .expect("fixture written");
}

#[test]
fn a_literal_maths_symbol_inside_mathtex_is_refused_with_the_macro_to_use() {
    // What a Spanish-language explainer writes every time. LaTeX reads bytes, so
    // a literal sigma is an undefined character, and the failure otherwise
    // surfaces only when Manim typesets the formula — costing a repair round for
    // something that can be seen by reading.
    let source = "from manim import *\n\n\
         class Scene_intro(Scene):\n\
         \x20   def construct(self):\n\
         \x20       label = MathTex(r\"σ + jω\")\n\
         \x20       self.play(Write(label), run_time=1.0)\n\
         \x20       self.wait(1.0)\n";
    let error = validate_scene_module("intro", source).expect_err("a literal sigma is refused");
    assert!(error.contains('σ'), "{error}");
    assert!(error.contains("\\sigma"), "{error}");
}

#[test]
fn a_maths_operator_that_latex_cannot_set_is_refused_too() {
    let source = format!("{VALID_SCENE}        note = MathTex(r\"t ≥ 0\")\n");
    assert!(validate_scene_module("intro", &source).is_err());
}

#[test]
fn accented_prose_and_unicode_outside_maths_are_left_alone() {
    // `\text{región}` sets correctly and a Text mobject is drawn with a font
    // rather than LaTeX, so refusing either would break every explainer not
    // written in English.
    let source = "from manim import *\n\n\
         class Scene_intro(Scene):\n\
         \x20   def construct(self):\n\
         \x20       titulo = Text(\"σ controla la región de convergencia\")\n\
         \x20       formula = MathTex(r\"\\text{integración} \\quad \\sigma > 0\")\n\
         \x20       self.play(Write(titulo), Write(formula), run_time=2.0)\n\
         \x20       self.wait(1.0)\n";
    validate_scene_module("intro", source).expect("prose and macros are fine");
}

#[test]
fn an_identifier_that_merely_ends_in_tex_is_not_mistaken_for_a_math_mobject() {
    let source = format!("{VALID_SCENE}        vertex = latex(\"σ\")\n");
    validate_scene_module("intro", &source).expect("only Tex/MathTex reach LaTeX");
}
