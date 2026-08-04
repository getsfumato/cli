//! Filesystem and generated-image tools exposed to text models.

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{
        ImageGenerationRequest, SpeechGenerationRequest, ToolDefinition, ToolExecutionRequest,
        ToolExecutor, ToolFunctionDefinition, VideoGenerationRequest,
    },
    python::{PythonRunRequest, screen_python_source},
    resources::narration::{audio_extension, audio_media_type},
    themes::ThemePackage,
    tools::{
        AudioToolConfig, ChartToolConfig, GenerationToolFactory, GenerationToolsRequest,
        ImageToolConfig, ToolSet, VideoToolConfig,
    },
};
use sha2::{Digest, Sha256};

const MAX_DIRECTORY_ENTRIES: usize = 200;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_GENERATED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_GENERATED_VIDEO_BYTES: usize = 512 * 1024 * 1024;
const MAX_GENERATED_AUDIO_BYTES: usize = 64 * 1024 * 1024;
/// Longest passage one tool call may speak, so a runaway request is refused
/// before it is billed rather than after.
const MAX_SPOKEN_CHARACTERS: usize = 5_000;

/// Builds filesystem-backed tools for one generation operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemGenerationToolFactory;

#[derive(Clone, Debug)]
struct FilesystemToolExecutor {
    roots: Vec<PathBuf>,
    max_file_bytes: u64,
}

impl FilesystemToolExecutor {
    fn new(roots: Vec<PathBuf>) -> Result<Self> {
        let mut canonical_roots = Vec::new();
        for root in roots {
            let root = root
                .canonicalize()
                .with_context(|| format!("Could not resolve tool root {}", root.display()))?;
            if !canonical_roots.contains(&root) {
                canonical_roots.push(root);
            }
        }
        if canonical_roots.is_empty() {
            bail!("Filesystem tools need at least one readable root");
        }
        Ok(Self {
            roots: canonical_roots,
            max_file_bytes: MAX_FILE_BYTES,
        })
    }

    fn resolve_allowed_path(&self, requested: &str) -> Result<PathBuf> {
        if requested.trim().is_empty() {
            bail!("Tool path cannot be empty");
        }
        let path = PathBuf::from(requested);
        let candidate = if path.is_absolute() {
            path
        } else {
            self.roots[0].join(path)
        };
        let canonical = candidate
            .canonicalize()
            .with_context(|| format!("Could not resolve tool path {}", candidate.display()))?;
        if !self.roots.iter().any(|root| canonical.starts_with(root)) {
            bail!(
                "Refusing to read {} because it is outside the allowed generation roots",
                canonical.display()
            );
        }
        Ok(canonical)
    }

    fn list_directory(&self, path: &str) -> Result<String> {
        let path = self.resolve_allowed_path(path)?;
        if !path.is_dir() {
            bail!("{} is not a directory", path.display());
        }
        let mut entries = fs::read_dir(&path)
            .with_context(|| format!("Could not read directory {}", path.display()))?
            .map(|entry| {
                let entry = entry?;
                let metadata = entry.metadata()?;
                Ok(DirectoryEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: entry.path().display().to_string(),
                    kind: if metadata.is_dir() {
                        "directory"
                    } else {
                        "file"
                    }
                    .to_string(),
                    bytes: metadata.is_file().then_some(metadata.len()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let truncated = entries.len() > MAX_DIRECTORY_ENTRIES;
        entries.truncate(MAX_DIRECTORY_ENTRIES);
        serde_json::to_string(&json!({
            "path": path,
            "entries": entries,
            "truncated": truncated,
        }))
        .context("Could not serialize directory listing")
    }

    fn read_file(&self, path: &str) -> Result<String> {
        let path = self.resolve_allowed_path(path)?;
        if !path.is_file() {
            bail!("{} is not a file", path.display());
        }
        let metadata = path
            .metadata()
            .with_context(|| format!("Could not inspect {}", path.display()))?;
        if metadata.len() > self.max_file_bytes {
            bail!(
                "{} is {} bytes; the current read limit is {} bytes",
                path.display(),
                metadata.len(),
                self.max_file_bytes
            );
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        serde_json::to_string(&json!({
            "path": path,
            "content": content,
        }))
        .context("Could not serialize file content")
    }
}

#[async_trait]
impl ToolExecutor for FilesystemToolExecutor {
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String> {
        operation.checkpoint(stage)?;
        let result: Result<String> = (|| match request.name.as_str() {
            "sfumato_list_directory" => {
                let path = string_arg(&request.arguments, "path")?;
                self.list_directory(&path)
            }
            "sfumato_read_file" => {
                let path = string_arg(&request.arguments, "path")?;
                self.read_file(&path)
            }
            _ => bail!("Unknown Sfumato tool '{}'", request.name),
        })();
        result.map_err(|error| tool_error(error, stage))
    }
}

struct ImageGenerationTool {
    config: ImageToolConfig,
    prompt_catalog: Arc<dyn PromptCatalog>,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
    prompts: Arc<Mutex<Vec<PromptProvenance>>>,
}

#[derive(Serialize)]
struct ImagePromptContext<'a> {
    requested_prompt: &'a str,
    theme_name: &'a str,
    theme_colors: String,
    theme_fonts: String,
    project_instructions: &'a str,
}

impl ImageGenerationTool {
    async fn execute(
        &self,
        arguments: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        operation.checkpoint(stage)?;
        let prompt = string_arg(arguments, "prompt")?;
        let alt_text = optional_string_arg(arguments, "alt_text")?
            .unwrap_or_else(|| "Generated educational illustration".to_string());
        let context = ImagePromptContext {
            requested_prompt: &prompt,
            theme_name: &self.config.theme.manifest.name,
            theme_colors: format_tokens(&self.config.theme.manifest.tokens.colors),
            theme_fonts: format_tokens(&self.config.theme.manifest.tokens.fonts),
            project_instructions: self
                .config
                .project_instructions
                .as_deref()
                .unwrap_or_default(),
        };
        let rendered = self.prompt_catalog.render(PromptRenderRequest {
            id: PromptId::ImageGenerationUser,
            variables: PromptVariables::from_serializable(&context)?,
        })?;
        self.prompts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated image prompt registry is unavailable"))?
            .push(rendered.provenance);
        let response = self
            .config
            .provider
            .generate_image(
                ImageGenerationRequest {
                    prompt: rendered.text,
                },
                operation,
                stage,
            )
            .await?;
        operation.checkpoint(stage)?;
        if response.bytes.len() > MAX_GENERATED_IMAGE_BYTES {
            bail!(
                "Generated image is {} bytes; the current limit is {} bytes",
                response.bytes.len(),
                MAX_GENERATED_IMAGE_BYTES
            );
        }
        let extension = image_extension(&response.media_type)?;
        let content_hash = format!("{:x}", Sha256::digest(&response.bytes));
        let filename = format!("image-{}.{extension}", &content_hash[..24]);
        let path = self.config.output_dir.join(&filename);
        fs::create_dir_all(&self.config.output_dir)
            .with_context(|| format!("Could not create {}", self.config.output_dir.display()))?;
        fs::write(&path, response.bytes)
            .with_context(|| format!("Could not write generated image {}", path.display()))?;
        self.artifacts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated image artifact registry is unavailable"))?
            .push(path.clone());
        Ok(json!({
            "path": path,
            "markdown_path": format!("{}/{filename}", self.config.reference_prefix),
            "alt_text": alt_text,
            "media_type": response.media_type,
            "model_profile": self.config.profile_name,
        })
        .to_string())
    }
}

struct GenerationToolExecutor {
    filesystem: FilesystemToolExecutor,
    image: Option<ImageGenerationTool>,
    video: Option<VideoGenerationTool>,
    audio: Option<AudioGenerationTool>,
    chart: Option<ChartGenerationTool>,
}

/// Plots data locally by running model-written matplotlib code.
///
/// The model supplies only the plotting body. Sfumato owns the imports, the
/// non-interactive backend, the theme styling, the figure size, and the save,
/// because those are the parts that decide whether the picture matches the rest
/// of the resource and whether it renders at all on a machine with no display.
struct ChartGenerationTool {
    config: ChartToolConfig,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
}

/// Statements the caller owns, which a plotting body must not repeat.
///
/// Each of these would override a decision made for the whole resource — the
/// backend, the theme, where the file lands — so a body containing one is
/// refused with an explanation rather than silently overridden.
const RESERVED_CHART_STATEMENTS: [(&str, &str); 5] = [
    (
        "savefig",
        "Sfumato saves the figure; end with the plot, not a save",
    ),
    ("plt.show", "there is no display to show a figure on"),
    (
        "matplotlib.use",
        "the rendering backend is selected by Sfumato",
    ),
    (
        "plt.style.use",
        "the project theme already styles the figure",
    ),
    ("rcparams", "the project theme already styles the figure"),
];

/// The five roles a chart needs, resolved from whatever a theme happens to name.
///
/// Themes are free-form: the bundled one says `background`/`text`, another says
/// `canvas`/`ink`, a third omits a role entirely. Reading fixed key names and
/// falling back to fixed hex values meant a dark theme silently produced a white
/// chart, and the only fix would have been editing this file — which defeats the
/// point of configuring a theme once. So each role is resolved through a chain of
/// names themes actually use, and anything still missing is derived from the
/// colours that were found rather than guessed.
struct ChartPalette {
    background: String,
    text: String,
    muted: String,
    primary: String,
    accent: String,
    family: String,
}

impl ChartPalette {
    fn from_theme(theme: &ThemePackage) -> Self {
        let tokens = &theme.manifest.tokens;
        // `surface-card` and `background_hard` are the same convention spelled two
        // ways, so lookup ignores the separator.
        let normalise = |name: &str| name.trim().to_ascii_lowercase().replace('-', "_");
        let colours = tokens
            .colors
            .iter()
            .map(|(name, value)| (normalise(name), value.trim().to_string()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let role = |candidates: &[&str]| {
            candidates
                .iter()
                .find_map(|candidate| colours.get(&normalise(candidate)).cloned())
        };

        let background = role(&[
            "background",
            "canvas",
            "surface",
            "background_hard",
            "background_soft",
            "surface_card",
        ])
        .unwrap_or_else(|| "#ffffff".to_string());
        let mut text = role(&[
            "text",
            "ink",
            "text_strong",
            "body_strong",
            "on_dark",
            "body",
        ])
        .unwrap_or_else(|| readable_on(&background));
        // A theme may name a text colour meant for a different surface. Chart text
        // that cannot be read against the chart's own background is a defect no
        // matter which token it came from.
        if contrast_ratio(&text, &background) < 3.0 {
            text = readable_on(&background);
        }
        let muted = role(&["muted", "muted_soft", "hairline", "body", "text_muted"])
            .unwrap_or_else(|| blend(&text, &background, 0.45));
        let primary = role(&["primary", "accent"]).unwrap_or_else(|| text.clone());
        let accent = role(&[
            "accent",
            "accent_yellow",
            "primary_hover",
            "primary_active",
            "secondary",
        ])
        .unwrap_or_else(|| primary.clone());
        // Only the family name is useful to matplotlib; a CSS stack's fallbacks
        // are resolved by a browser, not by a font manager.
        let family = tokens
            .fonts
            .get("body")
            .and_then(|stack| stack.split(',').next())
            .map(|family| family.trim().trim_matches(['"', '\'']).to_string())
            .filter(|family| !family.is_empty())
            .unwrap_or_else(|| "DejaVu Sans".to_string());

        Self {
            background,
            text,
            muted,
            primary,
            accent,
            family,
        }
    }
}

/// Parses `#rgb` or `#rrggbb` into channels.
///
/// Rejects a non-ASCII value up front rather than byte-slicing it. `hex.len()`
/// counts bytes, so `#abcñd` used to pass the length check and then panic on a
/// slice landing inside the `ñ`. Theme colours are validated on load now, but
/// this is called with values from several sources and must not be the thing
/// that turns a bad colour into a crash.
fn channels(colour: &str) -> Option<(f64, f64, f64)> {
    let hex = colour.trim().strip_prefix('#')?;
    if !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    let expand = |value: &str| u8::from_str_radix(value, 16).ok().map(f64::from);
    match hex.len() {
        3 => {
            let mut digits = hex.chars().map(|digit| expand(&format!("{digit}{digit}")));
            Some((digits.next()??, digits.next()??, digits.next()??))
        }
        6 | 8 => Some((
            expand(&hex[0..2])?,
            expand(&hex[2..4])?,
            expand(&hex[4..6])?,
        )),
        _ => None,
    }
}

/// Relative luminance per WCAG, used to decide readability.
fn luminance(colour: &str) -> f64 {
    let Some((red, green, blue)) = channels(colour) else {
        // An unparseable colour is treated as light, which is the safer guess: it
        // yields dark text, legible on most surfaces.
        return 1.0;
    };
    let channel = |value: f64| {
        let value = value / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
}

fn contrast_ratio(left: &str, right: &str) -> f64 {
    let (lighter, darker) = {
        let (left, right) = (luminance(left), luminance(right));
        if left >= right {
            (left, right)
        } else {
            (right, left)
        }
    };
    (lighter + 0.05) / (darker + 0.05)
}

/// The near-black or near-white that reads against a surface.
fn readable_on(background: &str) -> String {
    if luminance(background) > 0.4 {
        "#101010".to_string()
    } else {
        "#f5f5f5".to_string()
    }
}

/// Mixes two colours, used to derive a muted tone a theme did not declare.
fn blend(colour: &str, towards: &str, amount: f64) -> String {
    let (Some(from), Some(to)) = (channels(colour), channels(towards)) else {
        return colour.to_string();
    };
    let mix = |from: f64, to: f64| (from + (to - from) * amount).round().clamp(0.0, 255.0) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(from.0, to.0),
        mix(from.1, to.1),
        mix(from.2, to.2)
    )
}

impl ChartGenerationTool {
    /// Wraps the model's plotting body in the parts Sfumato owns.
    ///
    /// Theme colours are pushed through `rcParams` rather than described in
    /// prose, so a chart matches the deck it sits in without the model having to
    /// be told the palette and remember to apply it.
    fn program(&self, body: &str, width: f64, height: f64) -> String {
        let palette = ChartPalette::from_theme(&self.config.theme);
        let ChartPalette {
            background,
            text,
            muted,
            primary,
            accent,
            family,
        } = palette;
        format!(
            r#"import logging
import matplotlib

matplotlib.use("Agg")
# A theme names web fonts, which a plotting environment has no reason to carry.
# Falling back to the bundled family is the intended behaviour, so it is not
# worth eighty warning lines that would crowd out a real traceback.
logging.getLogger("matplotlib.font_manager").setLevel(logging.ERROR)
import matplotlib.pyplot as plt
from matplotlib import cycler
import numpy as np

plt.rcParams.update({{
    "figure.figsize": ({width}, {height}),
    "figure.dpi": 200,
    "figure.facecolor": "{background}",
    "axes.facecolor": "{background}",
    "axes.edgecolor": "{muted}",
    "axes.labelcolor": "{text}",
    "axes.titlecolor": "{text}",
    "axes.prop_cycle": cycler(color=["{primary}", "{accent}", "{muted}", "{text}"]),
    "text.color": "{text}",
    "xtick.color": "{muted}",
    "ytick.color": "{muted}",
    "grid.color": "{muted}",
    "grid.alpha": 0.25,
    "font.family": ["{family}", "DejaVu Sans"],
    "savefig.facecolor": "{background}",
    "savefig.bbox": "tight",
}})

{body}

plt.savefig("chart.png")
"#
        )
    }

    async fn execute(
        &self,
        arguments: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        operation.checkpoint(stage)?;
        let code = string_arg(arguments, "code")?;
        if code.trim().is_empty() {
            bail!("sfumato_chart_gen needs matplotlib code to run");
        }
        let alt_text = string_arg(arguments, "alt_text")?;
        let lowercase = code.to_ascii_lowercase();
        for (statement, reason) in RESERVED_CHART_STATEMENTS {
            if lowercase.contains(statement) {
                bail!("Remove `{statement}` from the chart code: {reason}.");
            }
        }
        // Authorised first, so the screen can allow a package the project has
        // deliberately layered in without letting an unauthorised one through.
        let packages = optional_string_list_arg(arguments, "packages")?;
        for requirement in &packages {
            self.config
                .security
                .authorize_python_package(requirement)
                .map_err(|error| anyhow::anyhow!("{error}"))?;
        }
        screen_python_source(&code, &self.config.security.python_packages)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let width = optional_f64_arg(arguments, "width_inches")?.unwrap_or(8.0);
        let height = optional_f64_arg(arguments, "height_inches")?.unwrap_or(4.5);
        for (name, value) in [("width_inches", width), ("height_inches", height)] {
            if !(2.0..=20.0).contains(&value) {
                bail!("{name} must be between 2 and 20 inches; received {value}");
            }
        }

        let mut files = BTreeMap::new();
        files.insert("chart.py".to_string(), self.program(&code, width, height));
        let result = self
            .config
            .python
            .run(
                PythonRunRequest {
                    environment: "charting".to_string(),
                    extra_packages: packages,
                    files,
                    entrypoint: "chart.py".to_string(),
                    arguments: Vec::new(),
                    outputs: vec!["chart.png".to_string()],
                    output_dir: self.config.output_dir.clone(),
                },
                operation,
            )
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let produced = result
            .outputs
            .first()
            .context("Chart run reported no output")?;

        // Named by content hash like a generated image, so two identical charts
        // are one file and the unreferenced-asset sweep treats both the same way.
        let bytes = fs::read(produced)
            .with_context(|| format!("Could not read rendered chart {}", produced.display()))?;
        let content_hash = format!("{:x}", Sha256::digest(&bytes));
        let filename = format!("chart-{}.png", &content_hash[..24]);
        let path = self.config.output_dir.join(&filename);
        if path != *produced {
            fs::rename(produced, &path)
                .with_context(|| format!("Could not name rendered chart {}", path.display()))?;
        }
        // Registered like a generated image so a chart the draft never referenced
        // is swept up with the rest instead of lingering in the revision.
        self.artifacts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated chart artifact registry is unavailable"))?
            .push(path.clone());
        Ok(json!({
            "path": path,
            "markdown_path": format!("{}/{filename}", self.config.reference_prefix),
            "alt_text": alt_text,
            "media_type": "image/png",
        })
        .to_string())
    }
}

/// Speaks text and returns the audio plus the word timings behind it.
///
/// The timings are written beside the audio rather than returned inline: a
/// paragraph produces hundreds of them, which would crowd out the rest of the
/// model's context for a file it can read when it actually needs to caption.
struct AudioGenerationTool {
    config: AudioToolConfig,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
}

impl AudioGenerationTool {
    async fn execute(
        &self,
        arguments: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        operation.checkpoint(stage)?;
        let text = string_arg(arguments, "text")?;
        if text.trim().is_empty() {
            bail!("sfumato_audio_gen needs text to speak");
        }
        if text.chars().count() > MAX_SPOKEN_CHARACTERS {
            bail!(
                "Requested narration is {} characters; the per-call limit is {MAX_SPOKEN_CHARACTERS}",
                text.chars().count()
            );
        }
        let response = self
            .config
            .provider
            .generate_speech(
                SpeechGenerationRequest {
                    text: text.clone(),
                    voice: optional_string_arg(arguments, "voice")?
                        .or_else(|| self.config.options.voice.clone()),
                    previous_text: None,
                    next_text: None,
                },
                operation,
                stage,
            )
            .await?;
        operation.checkpoint(stage)?;
        if response.bytes.len() > MAX_GENERATED_AUDIO_BYTES {
            bail!(
                "Generated audio is {} bytes; the current limit is {MAX_GENERATED_AUDIO_BYTES} bytes",
                response.bytes.len()
            );
        }
        let format = self.config.options.output_format.as_deref();
        let extension = audio_extension(format);
        let content_hash = format!("{:x}", Sha256::digest(&response.bytes));
        let stem = format!("audio-{}", &content_hash[..24]);
        let filename = format!("{stem}.{extension}");
        fs::create_dir_all(&self.config.output_dir)
            .with_context(|| format!("Could not create {}", self.config.output_dir.display()))?;
        let path = self.config.output_dir.join(&filename);
        fs::write(&path, &response.bytes)
            .with_context(|| format!("Could not write generated audio {}", path.display()))?;
        self.artifacts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated audio artifact registry is unavailable"))?
            .push(path.clone());
        let mut timings_reference = None;
        if !response.words.is_empty() {
            let timings_name = format!("{stem}.words.json");
            let timings_path = self.config.output_dir.join(&timings_name);
            fs::write(&timings_path, serde_json::to_vec_pretty(&response.words)?)?;
            self.artifacts
                .lock()
                .map_err(|_| anyhow::anyhow!("Generated audio artifact registry is unavailable"))?
                .push(timings_path);
            timings_reference = Some(format!("{}/{timings_name}", self.config.reference_prefix));
        }
        Ok(json!({
            "path": path,
            "html_path": format!("{}/{filename}", self.config.reference_prefix),
            "word_timings_path": timings_reference,
            "duration_seconds": response.duration_seconds,
            "media_type": audio_media_type(format),
            "model_profile": self.config.profile_name,
        })
        .to_string())
    }
}

struct VideoGenerationTool {
    config: VideoToolConfig,
    prompt_catalog: Arc<dyn PromptCatalog>,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
    prompts: Arc<Mutex<Vec<PromptProvenance>>>,
    used: AtomicBool,
}

#[derive(Serialize)]
struct VideoPromptContext<'a> {
    requested_prompt: &'a str,
    accessible_description: &'a str,
    theme_name: &'a str,
    theme_colors: String,
    theme_fonts: String,
    project_instructions: &'a str,
}

impl VideoGenerationTool {
    async fn execute(
        &self,
        arguments: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        if self.used.swap(true, Ordering::AcqRel) {
            bail!("sfumato_video_gen can be called at most once per page generation");
        }
        let requested_prompt = string_arg(arguments, "prompt")?;
        let accessible_description = string_arg(arguments, "accessible_description")?;
        let context = VideoPromptContext {
            requested_prompt: &requested_prompt,
            accessible_description: &accessible_description,
            theme_name: &self.config.theme.manifest.name,
            theme_colors: format_tokens(&self.config.theme.manifest.tokens.colors),
            theme_fonts: format_tokens(&self.config.theme.manifest.tokens.fonts),
            project_instructions: self
                .config
                .project_instructions
                .as_deref()
                .unwrap_or_default(),
        };
        let rendered = self.prompt_catalog.render(PromptRenderRequest {
            id: PromptId::VideoGenerationUser,
            variables: PromptVariables::from_serializable(&context)?,
        })?;
        self.prompts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated video prompt registry is unavailable"))?
            .push(rendered.provenance);
        let options = &self.config.options;
        let response = self
            .config
            .provider
            .generate_video(
                VideoGenerationRequest {
                    prompt: rendered.text,
                    duration_seconds: options.duration_seconds.unwrap_or(5),
                    resolution: options.resolution.clone().unwrap_or_else(|| "720p".into()),
                    aspect_ratio: options
                        .aspect_ratio
                        .clone()
                        .unwrap_or_else(|| "16:9".into()),
                    generate_audio: options
                        .audio
                        .map(|audio| !matches!(audio, sfumato_core::config::VideoAudioMode::Off)),
                    seed: options.seed,
                    references: self.config.references.clone(),
                },
                operation,
                stage,
            )
            .await?;
        if response.bytes.len() > MAX_GENERATED_VIDEO_BYTES {
            bail!(
                "Generated video exceeds the {} byte limit",
                MAX_GENERATED_VIDEO_BYTES
            );
        }
        if response.media_type != "video/mp4" && response.media_type != "application/octet-stream" {
            bail!(
                "Unsupported generated video media type '{}'",
                response.media_type
            );
        }
        let content_hash = format!("{:x}", Sha256::digest(&response.bytes));
        let filename = format!("video-{}.mp4", &content_hash[..24]);
        let path = self.config.output_dir.join(&filename);
        fs::create_dir_all(&self.config.output_dir)?;
        fs::write(&path, response.bytes)?;
        self.artifacts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated video artifact registry is unavailable"))?
            .push(path.clone());
        Ok(json!({
            "path": path,
            "html_path": format!("{}/{filename}", self.config.reference_prefix),
            "accessible_description": accessible_description,
            "media_type": "video/mp4",
            "model_profile": self.config.profile_name,
        })
        .to_string())
    }
}

#[async_trait]
impl ToolExecutor for GenerationToolExecutor {
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String> {
        if request.name == "sfumato_image_gen" {
            return self
                .image
                .as_ref()
                .ok_or_else(|| {
                    SfumatoError::tool(
                        ErrorClass::Permanent,
                        "No image model is configured for this project",
                    )
                    .at_stage(stage)
                })?
                .execute(&request.arguments, operation, stage)
                .await
                .map_err(|error| tool_error(error, stage));
        }
        if request.name == "sfumato_audio_gen" {
            return self
                .audio
                .as_ref()
                .ok_or_else(|| {
                    SfumatoError::tool(
                        ErrorClass::Permanent,
                        "No speech model is configured for this project",
                    )
                    .at_stage(stage)
                })?
                .execute(&request.arguments, operation, stage)
                .await
                .map_err(|error| tool_error(error, stage));
        }
        if request.name == "sfumato_chart_gen" {
            return self
                .chart
                .as_ref()
                .ok_or_else(|| {
                    SfumatoError::tool(
                        ErrorClass::Permanent,
                        "Charting is not enabled for this project",
                    )
                    .at_stage(stage)
                })?
                .execute(&request.arguments, operation, stage)
                .await
                .map_err(|error| tool_error(error, stage));
        }
        if request.name == "sfumato_video_gen" {
            return self
                .video
                .as_ref()
                .ok_or_else(|| {
                    SfumatoError::tool(
                        ErrorClass::Permanent,
                        "No video model is configured for this page",
                    )
                    .at_stage(stage)
                })?
                .execute(&request.arguments, operation, stage)
                .await
                .map_err(|error| tool_error(error, stage));
        }
        self.filesystem.execute(request, operation, stage).await
    }
}

fn tool_error(error: anyhow::Error, stage: OperationStage) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        let mut error = error.clone();
        if error.stage.is_none() {
            error.stage = Some(stage);
        }
        return error;
    }
    SfumatoError::tool(ErrorClass::Permanent, format_args!("{error:#}")).at_stage(stage)
}

impl GenerationToolFactory for FilesystemGenerationToolFactory {
    fn create(&self, request: GenerationToolsRequest) -> SfumatoResult<ToolSet> {
        let result: Result<ToolSet> = (|| {
            let GenerationToolsRequest {
                project_root,
                sources,
                image,
                video,
                audio,
                chart,
                prompt_catalog,
            } = request;
            let mut roots = vec![project_root];
            for source in sources {
                if source.is_file() {
                    if let Some(parent) = source.parent() {
                        roots.push(parent.to_path_buf());
                    }
                } else {
                    roots.push(source);
                }
            }

            let artifacts = Arc::new(Mutex::new(Vec::new()));
            let rendered_descriptions = prompt_catalog.render(PromptRenderRequest {
                id: PromptId::ToolsGenerationDescriptions,
                variables: PromptVariables::default(),
            })?;
            let descriptions: ToolDescriptions = serde_json::from_str(&rendered_descriptions.text)
                .context("Generation tool description prompt must render a JSON object")?;
            let prompts = Arc::new(Mutex::new(vec![rendered_descriptions.provenance]));
            let mut definitions = vec![
                list_directory_tool(&descriptions),
                read_file_tool(&descriptions),
            ];
            let image = image.map(|config| {
                definitions.push(image_generation_tool(&descriptions));
                ImageGenerationTool {
                    config,
                    prompt_catalog: prompt_catalog.clone(),
                    artifacts: artifacts.clone(),
                    prompts: prompts.clone(),
                }
            });
            let video = video.map(|config| {
                definitions.push(video_generation_tool(&descriptions));
                VideoGenerationTool {
                    config,
                    prompt_catalog: prompt_catalog.clone(),
                    artifacts: artifacts.clone(),
                    prompts: prompts.clone(),
                    used: AtomicBool::new(false),
                }
            });
            let audio = audio.map(|config| {
                definitions.push(audio_generation_tool(&descriptions));
                AudioGenerationTool {
                    config,
                    artifacts: artifacts.clone(),
                }
            });
            let chart = chart.map(|config| {
                definitions.push(chart_generation_tool(&descriptions));
                ChartGenerationTool {
                    config,
                    artifacts: artifacts.clone(),
                }
            });
            Ok(ToolSet {
                definitions,
                executor: Arc::new(GenerationToolExecutor {
                    filesystem: FilesystemToolExecutor::new(roots)?,
                    image,
                    video,
                    audio,
                    chart,
                }),
                artifacts,
                prompts,
            })
        })();
        result.map_err(|error| SfumatoError::tool(ErrorClass::Permanent, format_args!("{error:#}")))
    }
}

#[derive(Serialize)]
struct DirectoryEntry {
    name: String,
    path: String,
    kind: String,
    bytes: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolDescriptions {
    list_directory: String,
    list_directory_path: String,
    read_file: String,
    read_file_path: String,
    image_generation: String,
    image_prompt: String,
    image_alt_text: String,
    video_generation: String,
    video_prompt: String,
    video_accessible_description: String,
    audio_generation: String,
    audio_text: String,
    audio_voice: String,
    chart_generation: String,
    chart_code: String,
    chart_alt_text: String,
    chart_packages: String,
    chart_size: String,
}

fn list_directory_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_list_directory".to_string(),
            description: descriptions.list_directory.clone(),
            parameters: json!({
                "type": "object",
                "properties": { "path": {
                    "type": "string",
                    "description": descriptions.list_directory_path
                }},
                "required": ["path"]
            }),
        },
    }
}

fn read_file_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: descriptions.read_file.clone(),
            parameters: json!({
                "type": "object",
                "properties": { "path": {
                    "type": "string",
                    "description": descriptions.read_file_path
                }},
                "required": ["path"]
            }),
        },
    }
}

fn image_generation_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_image_gen".to_string(),
            description: descriptions.image_generation.clone(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": descriptions.image_prompt
                    },
                    "alt_text": {
                        "type": "string",
                        "description": descriptions.image_alt_text
                    }
                },
                "required": ["prompt"]
            }),
        },
    }
}

fn chart_generation_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_chart_gen".to_string(),
            description: descriptions.chart_generation.clone(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": descriptions.chart_code },
                    "alt_text": { "type": "string", "description": descriptions.chart_alt_text },
                    "packages": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": descriptions.chart_packages
                    },
                    "width_inches": { "type": "number", "description": descriptions.chart_size },
                    "height_inches": { "type": "number", "description": descriptions.chart_size }
                },
                "required": ["code", "alt_text"]
            }),
        },
    }
}

fn video_generation_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_video_gen".to_string(),
            description: descriptions.video_generation.clone(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": descriptions.video_prompt },
                    "accessible_description": {
                        "type": "string",
                        "description": descriptions.video_accessible_description
                    }
                },
                "required": ["prompt", "accessible_description"]
            }),
        },
    }
}

fn audio_generation_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_audio_gen".to_string(),
            description: descriptions.audio_generation.clone(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": descriptions.audio_text },
                    "voice": { "type": "string", "description": descriptions.audio_voice }
                },
                "required": ["text"]
            }),
        },
    }
}

fn string_arg(arguments: &Value, key: &str) -> Result<String> {
    let arguments = normalized_arguments(arguments)?;
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .with_context(|| format!("Tool argument '{key}' must be a string"))
}

fn optional_string_arg(arguments: &Value, key: &str) -> Result<Option<String>> {
    let arguments = normalized_arguments(arguments)?;
    match arguments.get(key) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("Tool argument '{key}' must be a string"),
        None => Ok(None),
    }
}

fn optional_string_list_arg(arguments: &Value, key: &str) -> Result<Vec<String>> {
    let arguments = normalized_arguments(arguments)?;
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .with_context(|| format!("Tool argument '{key}' must be a list of strings"))
            })
            .collect(),
        Some(_) => bail!("Tool argument '{key}' must be a list of strings"),
    }
}

fn optional_f64_arg(arguments: &Value, key: &str) -> Result<Option<f64>> {
    let arguments = normalized_arguments(arguments)?;
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .map(Some)
            .with_context(|| format!("Tool argument '{key}' must be a number")),
    }
}

fn normalized_arguments(arguments: &Value) -> Result<Value> {
    match arguments {
        Value::String(raw) => serde_json::from_str(raw)
            .with_context(|| format!("Tool arguments were not valid JSON: {raw}")),
        other => Ok(other.clone()),
    }
}

fn image_extension(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok("png"),
        "image/jpeg" | "image/jpg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        other => bail!("Unsupported generated image media type '{other}'"),
    }
}

fn format_tokens(tokens: &std::collections::BTreeMap<String, String>) -> String {
    if tokens.is_empty() {
        return "unspecified".to_string();
    }
    tokens
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[path = "../tests/unit/tools.rs"]
mod tests;
