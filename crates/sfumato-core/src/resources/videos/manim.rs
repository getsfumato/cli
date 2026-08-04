//! Deterministic assembly of a Manim film's non-authored parts.
//!
//! Mirrors what `assembly` does for Hyperframe, for the same reason: the parts a
//! renderer enforces are mechanical, and asking a model to reproduce them on top
//! of doing the creative work is where authoring failures came from. Here that
//! means the scene order, each scene's window, where the narration sits on the
//! timeline, and the caption track. The model writes one `construct` per beat and
//! nothing else.

use serde::{Deserialize, Serialize};
use sfumato_domain::VideoPlanDocument;

use crate::{
    errors::{ErrorClass, SfumatoError, SfumatoResult},
    python::screen_python_source,
    resources::{narration::CaptionGroup, videos::assembly::NarrationLayer},
};

/// The file the deterministic timeline is written to.
pub(super) const MANIFEST_PATH: &str = "manifest.json";

/// The generated caption overlay module, written beside the scenes.
pub(super) const CAPTIONS_PATH: &str = "captions.py";

/// The class the caption overlay module defines.
pub(super) const CAPTIONS_CLASS: &str = "SfumatoCaptions";

/// The Python identifier derived from a scene ID.
///
/// Scene IDs are slugs and routinely contain hyphens, which are legal in a file
/// stem but not in a Python module or class name. Deriving both from one
/// sanitiser keeps the module, the class, and the manifest entry in agreement.
pub(super) fn scene_symbol(scene_id: &str) -> String {
    let sanitised = scene_id
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    // A module cannot start with a digit, and a plan is free to name a scene "01".
    if sanitised.starts_with(|value: char| value.is_ascii_digit()) {
        format!("s_{sanitised}")
    } else {
        sanitised
    }
}

/// The module path one scene's authored source lives at.
pub(super) fn scene_module_path(scene_id: &str) -> String {
    format!("scenes/{}.py", scene_symbol(scene_id))
}

/// The class name a scene's authored source must define.
pub(super) fn scene_class_name(scene_id: &str) -> String {
    format!("Scene_{}", scene_symbol(scene_id))
}

/// One scene's place on the rendered timeline.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ManimSceneEntry {
    /// Plan scene ID.
    pub id: String,
    /// Module rendered for this scene, relative to the source root.
    pub module: String,
    /// Class the renderer asks Manim for.
    pub class_name: String,
    /// Window the scene occupies, in seconds.
    pub duration_seconds: f32,
}

/// One narration file and where it sits on the timeline.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ManimAudioEntry {
    /// Path relative to the source root.
    pub reference: String,
    /// Start on the film's timeline, in seconds.
    pub start_seconds: f32,
}

/// Everything the renderer needs that the model did not write.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ManimManifest {
    /// Scenes in playback order.
    pub scenes: Vec<ManimSceneEntry>,
    /// Narration clips mixed under the concatenated video.
    pub audio: Vec<ManimAudioEntry>,
    /// Caption overlay composited over the film, when captions were generated.
    pub captions: Option<ManimCaptionEntry>,
}

/// The generated caption overlay and the class that draws it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ManimCaptionEntry {
    /// Module rendered with a transparent background, relative to the source root.
    pub module: String,
    /// Class the renderer asks Manim for.
    pub class_name: String,
}

/// Builds the timeline the renderer replays.
pub(super) fn manim_manifest(plan: &VideoPlanDocument, layer: &NarrationLayer) -> ManimManifest {
    ManimManifest {
        scenes: plan
            .scenes()
            .iter()
            .map(|scene| ManimSceneEntry {
                id: scene.id.clone(),
                module: scene_module_path(&scene.id),
                class_name: scene_class_name(&scene.id),
                duration_seconds: scene.duration_seconds,
            })
            .collect(),
        audio: layer
            .clips
            .iter()
            .map(|clip| ManimAudioEntry {
                reference: clip.reference.clone(),
                start_seconds: clip.start_seconds,
            })
            .collect(),
        captions: layer.captions.then(|| ManimCaptionEntry {
            module: CAPTIONS_PATH.to_string(),
            class_name: CAPTIONS_CLASS.to_string(),
        }),
    }
}

/// The film's full length: the last scene's window end.
///
/// Taken from the plan rather than summed, because a retimed film's scenes carry
/// the offsets the voice actually produced.
pub(super) fn film_seconds(plan: &VideoPlanDocument) -> f32 {
    plan.scenes()
        .iter()
        .map(|scene| scene.start_seconds + scene.duration_seconds)
        .fold(0.0_f32, f32::max)
}

/// Serialises the manifest for the source document.
pub(super) fn manim_manifest_json(
    plan: &VideoPlanDocument,
    layer: &NarrationLayer,
) -> SfumatoResult<String> {
    serde_json::to_string_pretty(&manim_manifest(plan, layer)).map_err(|error| {
        SfumatoError::internal(format!("Could not render Manim manifest: {error}"))
    })
}

/// Generates the Manim module that draws the caption overlay.
///
/// Captions are a separate transparent render composited over the finished film,
/// rather than drawn inside each scene or burned in from a subtitle file. Drawing
/// them per scene fails because a Manim scene has no notion of an overlay that
/// outlives it, so a caption crossing a scene boundary would have to be authored
/// into both and kept in sync by hand. Burning them from a subtitle file fails
/// because that needs an FFmpeg built with libass, which many installations —
/// including Homebrew's default — do not have. Rendering them with the Manim that
/// is already required, and compositing with FFmpeg's core `overlay`, depends on
/// nothing beyond what the engine already needs.
///
/// The module is generated, never authored: its timings are the ones the voice
/// actually produced.
pub(super) fn captions_module(groups: &[CaptionGroup], total_seconds: f32, height: u32) -> String {
    // Scaled from the canvas so a vertical cut and a widescreen one read the same.
    let font_size = (height as f32 * 0.042).round().max(18.0) as u32;
    // Serialised as JSON, which is also valid Python literal syntax for the
    // strings and numbers used here, so a caption carrying a quote or a backslash
    // cannot break out of the source it is embedded in.
    // Rounded to milliseconds, which is finer than any caption needs and keeps a
    // generated module readable instead of full of f32 representation noise.
    let millis = |seconds: f32| (f64::from(seconds.max(0.0)) * 1_000.0).round() / 1_000.0;
    let timed = groups
        .iter()
        .map(|group| {
            serde_json::json!([
                group.text,
                millis(group.start_seconds),
                millis(group.end_seconds.max(group.start_seconds))
            ])
        })
        .collect::<Vec<_>>();
    let groups_literal = serde_json::to_string(&timed).unwrap_or_else(|_| "[]".to_string());
    let total = millis(total_seconds);
    format!(
        r#"from manim import *

GROUPS = {groups_literal}
TOTAL = {total}
FONT_SIZE = {font_size}


class {CAPTIONS_CLASS}(Scene):
    def construct(self):
        cursor = 0.0
        for text, start, end in GROUPS:
            if start > cursor:
                self.wait(start - cursor)
            label = Text(text, font_size=FONT_SIZE, color=WHITE)
            label.to_edge(DOWN, buff=0.55)
            plate = SurroundingRectangle(
                label,
                color=BLACK,
                fill_color=BLACK,
                fill_opacity=0.55,
                stroke_width=0,
                buff=0.18,
            )
            self.add(plate, label)
            self.wait(max(end - start, 0.04))
            self.remove(plate, label)
            cursor = end
        # Held to the film's full length so the overlay and the picture end
        # together; a short overlay would simply stop compositing partway.
        if TOTAL > cursor:
            self.wait(TOTAL - cursor)
"#
    )
}

/// Statements a scene may not carry, and why the caller owns each.
///
/// Every one of these is a decision made for the whole film: how long a beat
/// runs, where the file lands, what the canvas is. A scene that sets one of them
/// would silently disagree with the manifest the renderer is replaying.
const RESERVED_SCENE_STATEMENTS: [(&str, &str); 4] = [
    (
        "config.",
        "Sfumato sets the canvas, frame rate, and output paths",
    ),
    (
        "add_sound",
        "narration is mixed onto the finished film, not into a scene",
    ),
    (
        "class Scene(",
        "the scene must subclass Manim's Scene, not redefine it",
    ),
    (
        "if __name__",
        "the renderer imports the module and names the class itself",
    ),
];

/// Rejects an authored scene module that would not render as planned.
pub(super) fn validate_scene_module(scene_id: &str, source: &str) -> Result<(), String> {
    let class = scene_class_name(scene_id);
    if !source.contains(&format!("class {class}(")) {
        return Err(format!(
            "scenes/{}.py must define `class {class}(Scene)`",
            scene_symbol(scene_id)
        ));
    }
    if !source.contains("def construct(self)") {
        return Err(format!("{class} must define `def construct(self):`"));
    }
    // Without a wait the scene renders as a handful of frames and the film's
    // audio runs on over a frozen picture, which reads as a broken render rather
    // than a short beat.
    if !source.contains("self.wait(") && !source.contains("self.play(") {
        return Err(format!(
            "{class}.construct must animate with self.play(...) or hold with self.wait(...)"
        ));
    }
    let lowercase = source.to_ascii_lowercase();
    for (statement, reason) in RESERVED_SCENE_STATEMENTS {
        if lowercase.contains(statement) {
            return Err(format!("remove `{statement}`: {reason}"));
        }
    }
    if let Some(complaint) = unicode_in_math(source) {
        return Err(complaint);
    }
    // No extra modules: the Manim environment installs exactly `manim`, and
    // unlike the chart tool this path never layers project packages on top, so
    // there is nothing beyond the base allowlist to permit.
    screen_python_source(source, &[]).map_err(|error| error.to_string())?;
    Ok(())
}

/// Reports a literal maths symbol inside a `MathTex`, which LaTeX cannot set.
///
/// `MathTex` and `Tex` hand their argument to LaTeX, which reads bytes, not
/// Unicode: a literal `σ` is an undefined character where `\sigma` is a letter.
/// A model writing an explainer in Spanish reaches for the real symbol every
/// time, and the failure only surfaces once Manim tries to typeset the formula —
/// costing a whole repair round for something that can be seen by reading. Text
/// mobjects are rendered with a font rather than LaTeX, so they are left alone.
fn unicode_in_math(source: &str) -> Option<String> {
    for (index, _) in source.match_indices("Tex(") {
        // `MathTex(`, `Tex(`, and `SingleStringMathTex(` all reach LaTeX; a name
        // merely ending in those letters, like `matex(`, does not.
        let head = &source[..index];
        if head
            .chars()
            .next_back()
            .is_some_and(|value| value.is_alphanumeric() || value == '_')
            && !head.ends_with("Math")
            && !head.ends_with("MathTex")
        {
            continue;
        }
        let argument = &source[index + "Tex(".len()..];
        let end = balanced_end(argument);
        if let Some(symbol) = argument[..end].chars().find(is_maths_symbol) {
            return Some(format!(
                "write `{symbol}` as a LaTeX macro inside Tex/MathTex — LaTeX cannot set a literal maths symbol. Use \\sigma, \\omega, \\ge and the like, or move the text into a Text(...) mobject, which is rendered with a font instead"
            ));
        }
    }
    None
}

/// Greek letters and the maths operator blocks, which have no LaTeX glyph.
///
/// Accented Latin is deliberately absent: `\text{integración}` sets correctly, and
/// refusing it would break every explainer not written in English.
fn is_maths_symbol(value: &char) -> bool {
    matches!(
        u32::from(*value),
        0x0370..=0x03FF     // Greek and Coptic, which is where σ and ω live
        | 0x1D400..=0x1D7FF // Mathematical alphanumeric symbols
        | 0x2200..=0x22FF   // Mathematical operators, including ∈, ≤ and ≥
        | 0x2A00..=0x2AFF   // Supplemental mathematical operators
    )
}

/// Where a call's argument list closes, ignoring parentheses inside strings.
fn balanced_end(argument: &str) -> usize {
    let mut depth = 1_i32;
    let mut quote: Option<char> = None;
    let mut previous = '\0';
    for (offset, value) in argument.char_indices() {
        match quote {
            Some(open) => {
                if value == open && previous != '\\' {
                    quote = None;
                }
            }
            None => match value {
                '"' | '\'' => quote = Some(value),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return offset;
                    }
                }
                _ => {}
            },
        }
        previous = value;
    }
    argument.len()
}

/// Strips a fenced code block a model wrapped its module in.
pub(super) fn strip_python_fence(response: &str) -> String {
    let trimmed = response.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    // The opening fence may carry a language tag; the body starts after that line.
    let body = rest.split_once('\n').map(|(_, body)| body).unwrap_or("");
    body.trim_end()
        .strip_suffix("```")
        .unwrap_or(body)
        .trim()
        .to_string()
}

/// Reports a Manim source that could not be assembled.
pub(super) fn manim_error(message: impl std::fmt::Display) -> SfumatoError {
    SfumatoError::render(ErrorClass::InvalidOutput, message)
}

#[cfg(test)]
#[path = "../../../tests/unit/resources_videos_manim.rs"]
mod tests;
