use super::*;
use crate::python::UvPythonRuntime;
use async_trait::async_trait;
use sfumato_core::{
    config::ProjectSecurityConfig,
    errors::{OperationStage, SfumatoResult},
    operation::OperationContext,
    prompts::{PromptError, PromptOrigin, PromptProvenance, RenderedPrompt},
    providers::{
        ImageGenerationProvider, ImageGenerationResponse, VideoGenerationProvider,
        VideoGenerationRequest, VideoGenerationResponse,
    },
    themes::{THEME_SCHEMA_VERSION, ThemeAdapters, ThemeManifest, ThemePackage, ThemeTokens},
};
use std::{collections::BTreeMap, sync::Mutex};

struct MockImageProvider {
    prompts: Arc<Mutex<Vec<String>>>,
}

struct MockVideoProvider {
    prompts: Arc<Mutex<Vec<String>>>,
}

struct TestPromptCatalog;

impl PromptCatalog for TestPromptCatalog {
    fn render(
        &self,
        request: PromptRenderRequest,
    ) -> std::result::Result<sfumato_core::prompts::RenderedPrompt, PromptError> {
        if request.id == PromptId::ToolsGenerationDescriptions {
            return Ok(RenderedPrompt {
                text: serde_json::json!({
                    "list_directory": "List a directory.",
                    "list_directory_path": "Directory path.",
                    "read_file": "Read a file.",
                    "read_file_path": "File path.",
                    "image_generation": "Generate an image.",
                    "image_prompt": "Image prompt.",
                    "image_alt_text": "Accessible alternative text.",
                    "video_generation": "Generate a video.",
                    "video_prompt": "Video prompt.",
                    "video_accessible_description": "Accessible video description.",
                    "audio_generation": "Speak a line of narration.",
                    "audio_text": "Words to speak.",
                    "audio_voice": "Voice identifier.",
                    "chart_generation": "Plot data with matplotlib.",
                    "chart_code": "Plotting statements.",
                    "chart_alt_text": "Accessible chart description.",
                    "chart_packages": "Extra Python packages.",
                    "chart_size": "Figure dimension in inches."
                })
                .to_string(),
                provenance: PromptProvenance {
                    id: request.id,
                    origin: PromptOrigin::Bundled,
                    version: 1,
                    content_hash: "test-tools".to_string(),
                },
            });
        }
        let value = |key: &str| {
            request.variables.0[key]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        Ok(RenderedPrompt {
            text: format!(
                "{}\nTheme: {}\nSemantic colors: {}\nTypography: {}\n{}",
                value("requested_prompt"),
                value("theme_name"),
                value("theme_colors"),
                value("theme_fonts"),
                value("project_instructions")
            ),
            provenance: PromptProvenance {
                id: request.id,
                origin: PromptOrigin::Bundled,
                version: 1,
                content_hash: "test".to_string(),
            },
        })
    }

    fn validate(&self) -> std::result::Result<Vec<PromptProvenance>, PromptError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ImageGenerationProvider for MockImageProvider {
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        self.prompts.lock().unwrap().push(request.prompt);
        Ok(ImageGenerationResponse {
            bytes: b"fake-png".to_vec(),
            media_type: "image/png".to_string(),
        })
    }
}

#[async_trait]
impl VideoGenerationProvider for MockVideoProvider {
    async fn generate_video(
        &self,
        request: VideoGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<VideoGenerationResponse> {
        self.prompts.lock().unwrap().push(request.prompt);
        Ok(VideoGenerationResponse {
            bytes: b"fake-mp4".to_vec(),
            media_type: "video/mp4".into(),
            provider_job_id: Some("job-1".into()),
            cost: Some(0.01),
        })
    }
}

#[tokio::test]
async fn lists_and_reads_inside_allowed_roots() {
    let temp = tempfile::tempdir().unwrap();
    let note = temp.path().join("note.md");
    fs::write(&note, "# Note").unwrap();
    let executor = FilesystemToolExecutor::new(vec![temp.path().to_path_buf()]).unwrap();

    let listing = executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_list_directory".to_string(),
                arguments: json!({ "path": temp.path() }),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    assert!(listing.contains("note.md"));

    let content = executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_read_file".to_string(),
                arguments: json!({ "path": note }),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    assert!(content.contains("# Note"));
}

#[tokio::test]
async fn rejects_paths_outside_allowed_roots() {
    let allowed = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.md");
    fs::write(&secret, "nope").unwrap();
    let executor = FilesystemToolExecutor::new(vec![allowed.path().to_path_buf()]).unwrap();

    assert!(
        executor
            .execute(
                ToolExecutionRequest {
                    name: "sfumato_read_file".to_string(),
                    arguments: json!({ "path": secret }),
                },
                &OperationContext::detached(),
                OperationStage::Draft
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn tool_arguments_accept_json_strings() {
    let temp = tempfile::tempdir().unwrap();
    let note = temp.path().join("note.md");
    fs::write(&note, "# Note").unwrap();
    let executor = FilesystemToolExecutor::new(vec![temp.path().to_path_buf()]).unwrap();

    let content = executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_read_file".to_string(),
                arguments: Value::String(format!(r#"{{"path":"{}"}}"#, note.display())),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    assert!(content.contains("# Note"));
}

#[tokio::test]
async fn image_tool_injects_theme_and_tracks_the_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let theme = ThemePackage {
        root: temp.path().to_path_buf(),
        manifest: ThemeManifest {
            schema_version: THEME_SCHEMA_VERSION,
            name: "gruvbox".to_string(),
            description: "Test".to_string(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([
                    ("background".to_string(), "#282828".to_string()),
                    ("accent".to_string(), "#fabd2f".to_string()),
                ]),
                fonts: BTreeMap::from([("body".to_string(), "Inter".to_string())]),
            },
            adapters: ThemeAdapters {
                marp_css: "marp/theme.css".into(),
                html: None,
                document: None,
            },
        },
    };
    let tools = FilesystemGenerationToolFactory
        .create(GenerationToolsRequest {
            project_root: temp.path().to_path_buf(),
            sources: Vec::new(),
            image: Some(ImageToolConfig {
                provider: Arc::new(MockImageProvider {
                    prompts: prompts.clone(),
                }),
                profile_name: "openrouter-image".to_string(),
                output_dir: temp.path().join("slides/images"),
                reference_prefix: "images".to_string(),
                theme,
                project_instructions: Some("Use Spanish labels.".to_string()),
            }),
            video: None,
            audio: None,
            chart: None,
            prompt_catalog: Arc::new(TestPromptCatalog),
        })
        .unwrap();

    assert!(
        tools
            .definitions
            .iter()
            .any(|tool| tool.function.name == "sfumato_image_gen")
    );
    let result = tools
        .executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_image_gen".to_string(),
                arguments: json!({
                    "prompt": "A labeled unit circle",
                    "alt_text": "Unit circle with sine and cosine"
                }),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    let result: Value = serde_json::from_str(&result).unwrap();

    let markdown_path = result["markdown_path"].as_str().unwrap();
    assert!(markdown_path.starts_with("images/image-"));
    assert!(markdown_path.ends_with(".png"));
    assert_eq!(tools.generated_artifacts().unwrap().len(), 1);
    assert_eq!(tools.generated_prompts().unwrap().len(), 2);
    assert!(tools.generated_artifacts().unwrap()[0].is_file());
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("Theme: gruvbox"));
    assert!(prompt.contains("background=#282828"));
    assert!(prompt.contains("A labeled unit circle"));
    assert!(prompt.contains("Use Spanish labels."));
}

#[test]
fn filesystem_only_tools_do_not_declare_image_generation() {
    let temp = tempfile::tempdir().unwrap();
    let tools = FilesystemGenerationToolFactory
        .create(GenerationToolsRequest {
            project_root: temp.path().to_path_buf(),
            sources: Vec::new(),
            image: None,
            video: None,
            audio: None,
            chart: None,
            prompt_catalog: Arc::new(TestPromptCatalog),
        })
        .unwrap();

    assert!(
        tools
            .definitions
            .iter()
            .all(|tool| tool.function.name != "sfumato_image_gen")
    );
}

#[tokio::test]
async fn page_video_tool_injects_theme_tracks_mp4_and_allows_one_call() {
    let temp = tempfile::tempdir().unwrap();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let theme = ThemePackage {
        root: temp.path().to_path_buf(),
        manifest: ThemeManifest {
            schema_version: THEME_SCHEMA_VERSION,
            name: "gruvbox".into(),
            description: "Test".into(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([("accent".into(), "#fabd2f".into())]),
                fonts: BTreeMap::from([("body".into(), "Inter".into())]),
            },
            adapters: ThemeAdapters {
                marp_css: "marp/theme.css".into(),
                html: None,
                document: None,
            },
        },
    };
    let tools = FilesystemGenerationToolFactory
        .create(GenerationToolsRequest {
            project_root: temp.path().to_path_buf(),
            sources: Vec::new(),
            image: None,
            video: Some(VideoToolConfig {
                provider: Arc::new(MockVideoProvider {
                    prompts: prompts.clone(),
                }),
                profile_name: "remote-video".into(),
                output_dir: temp.path().join("pages/assets/videos"),
                reference_prefix: "assets/videos".into(),
                theme,
                project_instructions: Some("Use Spanish labels.".into()),
                references: Vec::new(),
                options: Default::default(),
            }),
            audio: None,
            chart: None,
            prompt_catalog: Arc::new(TestPromptCatalog),
        })
        .unwrap();

    assert!(
        tools
            .definitions
            .iter()
            .any(|tool| tool.function.name == "sfumato_video_gen")
    );
    let request = ToolExecutionRequest {
        name: "sfumato_video_gen".into(),
        arguments: json!({
            "prompt": "Animate harmonic synthesis",
            "accessible_description": "Sine waves combine into a square wave"
        }),
    };
    let result = tools
        .executor
        .execute(
            request.clone(),
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    let result: Value = serde_json::from_str(&result).unwrap();

    assert!(
        result["html_path"]
            .as_str()
            .unwrap()
            .starts_with("assets/videos/video-")
    );
    assert_eq!(tools.generated_artifacts().unwrap().len(), 1);
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("Theme: gruvbox"));
    assert!(prompt.contains("Animate harmonic synthesis"));
    assert!(prompt.contains("Use Spanish labels."));

    let error = tools
        .executor
        .execute(
            request,
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("at most once"));
}

fn charting_theme(root: &std::path::Path) -> ThemePackage {
    ThemePackage {
        root: root.to_path_buf(),
        manifest: ThemeManifest {
            schema_version: THEME_SCHEMA_VERSION,
            name: "gruvbox".to_string(),
            description: "Test".to_string(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([
                    ("background".to_string(), "#282828".to_string()),
                    ("primary".to_string(), "#458588".to_string()),
                    ("accent".to_string(), "#fabd2f".to_string()),
                ]),
                fonts: BTreeMap::from([(
                    "body".to_string(),
                    "\"IBM Plex Sans\", Arial, sans-serif".to_string(),
                )]),
            },
            adapters: ThemeAdapters {
                marp_css: "marp/theme.css".into(),
                html: None,
                document: None,
            },
        },
    }
}

fn chart_tool(root: &std::path::Path, security: ProjectSecurityConfig) -> ChartGenerationTool {
    ChartGenerationTool {
        config: ChartToolConfig {
            python: Arc::new(UvPythonRuntime::new(root.join("python"))),
            output_dir: root.join("slides/images"),
            reference_prefix: "images".to_string(),
            theme: charting_theme(root),
            project_instructions: None,
            security,
        },
        artifacts: Arc::new(Mutex::new(Vec::new())),
    }
}

#[test]
fn the_generated_chart_program_carries_the_theme_and_owns_the_save() {
    let temp = tempfile::tempdir().unwrap();
    let tool = chart_tool(temp.path(), ProjectSecurityConfig::default());
    let program = tool.program("plt.plot([0, 1], [0, 1])", 8.0, 4.5);

    // The model is never told the palette; the figure inherits it regardless.
    assert!(program.contains("#282828"));
    assert!(program.contains("#458588"));
    // A CSS font stack's fallbacks mean nothing to a font manager, so only the
    // family survives.
    assert!(program.contains(r#""IBM Plex Sans", "DejaVu Sans"#));
    assert!(!program.contains("Arial, sans-serif"));
    // Headless by construction: the tool runs where there is no display.
    assert!(program.contains(r#"matplotlib.use("Agg")"#));
    assert!(program.contains("plt.plot([0, 1], [0, 1])"));
    assert!(program.contains(r#"plt.savefig("chart.png")"#));
    assert!(program.contains("(8, 4.5)"));
}

#[tokio::test]
async fn chart_code_that_reaches_past_plotting_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let tool = chart_tool(temp.path(), ProjectSecurityConfig::default());
    let operation = OperationContext::detached();

    for code in [
        // Each of these overrides a decision made for the whole resource.
        "plt.savefig('/tmp/escape.png')",
        "plt.show()",
        "matplotlib.use('TkAgg')",
        "plt.style.use('dark_background')",
        // And these leave the run directory entirely.
        "import os; os.system('id')",
        "open('/etc/passwd').read()",
    ] {
        let error = tool
            .execute(
                &json!({ "code": code, "alt_text": "x" }),
                &operation,
                OperationStage::Draft,
            )
            .await
            .expect_err(&format!("expected {code:?} to be refused"));
        assert!(!format!("{error:#}").is_empty());
    }
}

#[tokio::test]
async fn an_unpermitted_extra_package_is_refused_before_anything_is_installed() {
    let temp = tempfile::tempdir().unwrap();
    let tool = chart_tool(
        temp.path(),
        ProjectSecurityConfig {
            allow_python: true,
            python_packages: vec!["scipy".to_string()],
        },
    );
    let operation = OperationContext::detached();

    let error = tool
        .execute(
            &json!({ "code": "plt.plot([0, 1])", "alt_text": "x", "packages": ["requests"] }),
            &operation,
            OperationStage::Draft,
        )
        .await
        .expect_err("an unlisted package should be refused");
    assert!(format!("{error:#}").contains("requests"));
    assert!(
        !temp.path().join("python").exists(),
        "a refused package must not have provisioned an environment"
    );
}

#[tokio::test]
async fn an_out_of_range_figure_size_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let tool = chart_tool(temp.path(), ProjectSecurityConfig::default());
    let operation = OperationContext::detached();

    assert!(
        tool.execute(
            &json!({ "code": "plt.plot([0, 1])", "alt_text": "x", "width_inches": 400 }),
            &operation,
            OperationStage::Draft,
        )
        .await
        .is_err()
    );
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn a_rendered_chart_is_named_by_content_and_registered_as_an_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let tool = chart_tool(temp.path(), ProjectSecurityConfig::default());
    let operation = OperationContext::detached();

    let response = tool
        .execute(
            &json!({
                "code": "x = np.linspace(0, 10, 200)\nplt.plot(x, np.sin(x))\nplt.xlabel('t')",
                "alt_text": "A sine wave",
            }),
            &operation,
            OperationStage::Draft,
        )
        .await
        .expect("the chart should render");

    let payload: serde_json::Value = serde_json::from_str(&response).unwrap();
    let reference = payload["markdown_path"].as_str().unwrap();
    assert!(reference.starts_with("images/chart-"));
    assert!(reference.ends_with(".png"));
    assert_eq!(payload["alt_text"], "A sine wave");
    // The reference the model is handed must resolve to a file that exists, or
    // the draft will cite an image the resource cannot show.
    let produced = std::path::Path::new(payload["path"].as_str().unwrap());
    assert!(produced.is_file());
    assert_eq!(tool.artifacts.lock().unwrap().len(), 1);
}

fn theme_with(colors: &[(&str, &str)], font: &str) -> ThemePackage {
    ThemePackage {
        root: std::path::PathBuf::from("/themes/test"),
        manifest: ThemeManifest {
            schema_version: THEME_SCHEMA_VERSION,
            name: "test".to_string(),
            description: "Test".to_string(),
            tokens: ThemeTokens {
                colors: colors
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect(),
                fonts: BTreeMap::from([("body".to_string(), font.to_string())]),
            },
            adapters: ThemeAdapters {
                marp_css: "marp/theme.css".into(),
                html: None,
                document: None,
            },
        },
    }
}

#[test]
fn a_dark_theme_that_names_its_tokens_differently_still_yields_a_dark_chart() {
    // These are the Ferrari theme's real token names: no `background`, no `text`,
    // no `accent`. Reading fixed key names gave this theme a white chart, and the
    // only fix would have been editing the tool — which defeats configuring a
    // theme once and having everything follow it.
    let palette = ChartPalette::from_theme(&theme_with(
        &[
            ("canvas", "#181818"),
            ("ink", "#ffffff"),
            ("muted", "#666666"),
            ("primary", "#da291c"),
            ("accent-yellow", "#f6e500"),
        ],
        "'FerrariSans', sans-serif",
    ));

    assert_eq!(palette.background, "#181818");
    assert_eq!(palette.text, "#ffffff");
    assert_eq!(palette.muted, "#666666");
    assert_eq!(palette.primary, "#da291c");
    // The separator is not part of the convention, so `accent-yellow` answers for
    // an accent the theme never spelled exactly.
    assert_eq!(palette.accent, "#f6e500");
    // A CSS stack's quoting and fallbacks mean nothing to a font manager.
    assert_eq!(palette.family, "FerrariSans");
}

#[test]
fn a_theme_that_declares_only_a_background_still_produces_a_readable_chart() {
    let dark = ChartPalette::from_theme(&theme_with(&[("background", "#101820")], "Inter"));
    // Nothing to read but the background, so the rest is derived from it rather
    // than guessed: light text on a dark ground.
    assert!(contrast_ratio(&dark.text, &dark.background) > 7.0);
    assert!(contrast_ratio(&dark.muted, &dark.background) > 1.5);
    assert_eq!(dark.primary, dark.text);
    assert_eq!(dark.accent, dark.primary);

    let light = ChartPalette::from_theme(&theme_with(&[("background", "#fbf1c7")], "Inter"));
    assert!(contrast_ratio(&light.text, &light.background) > 7.0);
    assert_ne!(light.text, dark.text);
}

#[test]
fn text_that_cannot_be_read_against_the_chart_background_is_replaced() {
    // A theme may name a text colour meant for a different surface. Honouring it
    // here would produce an unreadable chart, so the background wins.
    let palette = ChartPalette::from_theme(&theme_with(
        &[("background", "#101820"), ("text", "#131b24")],
        "Inter",
    ));
    assert_ne!(palette.text, "#131b24");
    assert!(contrast_ratio(&palette.text, &palette.background) > 4.5);
}

#[test]
fn a_theme_with_no_colours_at_all_still_renders() {
    let palette = ChartPalette::from_theme(&theme_with(&[], ""));
    assert_eq!(palette.background, "#ffffff");
    assert!(contrast_ratio(&palette.text, &palette.background) > 7.0);
    assert_eq!(palette.family, "DejaVu Sans");
}

#[test]
fn every_installed_theme_yields_a_legible_chart_palette() {
    // Guards the property the whole mechanism exists for: switch the project theme
    // and charts follow, with no code change and no unreadable result.
    let themes = [
        ("sfumato-default", "#f7f7f5", "#202124"),
        ("gruvbox", "#fbf1c7", "#3c3836"),
        ("ferrari", "#181818", "#ffffff"),
    ];
    for (name, background, text) in themes {
        let palette = ChartPalette::from_theme(&theme_with(
            &match name {
                "sfumato-default" => vec![
                    ("background", "#f7f7f5"),
                    ("text", "#202124"),
                    ("muted", "#5f6368"),
                    ("primary", "#315c8c"),
                    ("accent", "#b35c24"),
                ],
                "gruvbox" => vec![
                    ("background", "#fbf1c7"),
                    ("text", "#3c3836"),
                    ("muted", "#7c6f64"),
                    ("primary", "#9d0006"),
                    ("accent", "#af3a03"),
                ],
                _ => vec![
                    ("canvas", "#181818"),
                    ("ink", "#ffffff"),
                    ("muted", "#666666"),
                    ("primary", "#da291c"),
                    ("accent-yellow", "#f6e500"),
                ],
            },
            "Inter",
        ));
        assert_eq!(palette.background, background, "{name} background");
        assert_eq!(palette.text, text, "{name} text");
        assert!(
            contrast_ratio(&palette.text, &palette.background) > 4.5,
            "{name} text is unreadable on its own background"
        );
    }
}

/// Emits the exact program the tool would run, per chart and theme.
///
/// Exists so the rendered output can be inspected by eye; it asserts nothing, and
/// deliberately goes through `program` rather than a copy of it so what is
/// inspected is what the tool actually runs.
#[cfg(feature = "real-renderers")]
#[test]
fn emit_chart_program_fixtures() {
    let charts: [(&str, &str); 3] = [
        (
            "roc",
            "fig, ax = plt.subplots()\n\
             ax.axvspan(1.0, 4, alpha=0.22, label='ROC: Re(s) > 1')\n\
             ax.axvline(1.0, lw=2, label='abscisa de convergencia')\n\
             ax.plot([1.0], [0.0], 'x', markersize=13, markeredgewidth=3)\n\
             ax.set_xlabel(r'$\\sigma = \\mathrm{Re}(s)$')\n\
             ax.set_ylabel(r'$\\omega = \\mathrm{Im}(s)$')\n\
             ax.set_title(r'Region de convergencia de $\\mathcal{L}\\{e^{t}\\}$')\n\
             ax.grid(True); ax.legend(loc='upper left')\n\
             ax.set_xlim(-4, 4); ax.set_ylim(-4, 4)",
        ),
        (
            "escalon",
            "t = np.linspace(0, 14, 600)\n\
             wn = 1.0\n\
             fig, ax = plt.subplots()\n\
             for z in (0.2, 0.5, 1.0):\n\
             \x20   if z < 1:\n\
             \x20       wd = wn*np.sqrt(1-z**2)\n\
             \x20       y = 1 - np.exp(-z*wn*t)*(np.cos(wd*t) + (z/np.sqrt(1-z**2))*np.sin(wd*t))\n\
             \x20   else:\n\
             \x20       y = 1 - np.exp(-wn*t)*(1 + wn*t)\n\
             \x20   ax.plot(t, y, label=rf'$\\zeta={z}$')\n\
             ax.axhline(1.0, ls='--', lw=1)\n\
             ax.set_xlabel('t [s]'); ax.set_ylabel('y(t)')\n\
             ax.set_title(r'Respuesta al escalon de $\\frac{\\omega_n^2}{s^2+2\\zeta\\omega_n s+\\omega_n^2}$')\n\
             ax.grid(True); ax.legend()",
        ),
        (
            "polos",
            "fig, ax = plt.subplots()\n\
             polos = np.array([-0.5+2j, -0.5-2j, -3.0+0j])\n\
             ceros = np.array([-1.5+0j])\n\
             ax.scatter(polos.real, polos.imag, marker='x', s=170, linewidths=3, label='polos')\n\
             ax.scatter(ceros.real, ceros.imag, marker='o', s=130, facecolors='none', linewidths=2.5, label='ceros')\n\
             ax.axhline(0, lw=1); ax.axvline(0, lw=1)\n\
             ax.set_xlabel(r'$\\sigma$'); ax.set_ylabel(r'$j\\omega$')\n\
             ax.set_title('Polos y ceros en el plano s')\n\
             ax.grid(True); ax.legend(loc='upper left')\n\
             ax.set_xlim(-4, 1); ax.set_ylim(-3, 3)",
        ),
    ];
    let themes: [(&str, Vec<(&str, &str)>); 3] = [
        (
            "sfumato-default",
            vec![
                ("background", "#f7f7f5"),
                ("text", "#202124"),
                ("muted", "#5f6368"),
                ("primary", "#315c8c"),
                ("accent", "#b35c24"),
            ],
        ),
        (
            "gruvbox",
            vec![
                ("background", "#fbf1c7"),
                ("text", "#3c3836"),
                ("muted", "#7c6f64"),
                ("primary", "#9d0006"),
                ("accent", "#af3a03"),
            ],
        ),
        (
            "ferrari",
            vec![
                ("canvas", "#181818"),
                ("ink", "#ffffff"),
                ("muted", "#666666"),
                ("primary", "#da291c"),
                ("accent-yellow", "#f6e500"),
            ],
        ),
    ];
    for (theme_name, colors) in &themes {
        let tool = ChartGenerationTool {
            config: ChartToolConfig {
                python: Arc::new(UvPythonRuntime::new(std::path::PathBuf::from("/unused"))),
                output_dir: std::path::PathBuf::from("/unused"),
                reference_prefix: "images".to_string(),
                theme: theme_with(colors, "Inter"),
                project_instructions: None,
                security: ProjectSecurityConfig::default(),
            },
            artifacts: Arc::new(Mutex::new(Vec::new())),
        };
        for (chart_name, body) in &charts {
            std::fs::write(
                std::env::temp_dir().join(format!("sfumato-chart-{theme_name}-{chart_name}.py")),
                tool.program(body, 7.0, 4.0),
            )
            .expect("fixture written");
        }
    }
}
