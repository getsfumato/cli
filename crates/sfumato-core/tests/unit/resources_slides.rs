use super::*;

use crate::{
    config::GlobalConfig,
    prompts::{PromptError, PromptOrigin, RenderedPrompt},
    providers::TextGenerationLimitError,
    sources::SourceDocument,
    themes::{THEME_SCHEMA_VERSION, ThemeAdapters, ThemeManifest},
};

fn effective_config() -> EffectiveConfig {
    let global = GlobalConfig::default_config();
    EffectiveConfig {
        user: global.user,
        project_name: "demo".to_string(),
        project_root: PathBuf::from("/tmp/demo"),
        publish_dir: None,
        theme: "sfumato-default".to_string(),
        connectors: global.connectors,
        models: global.models,
        model_defaults: global.defaults.0,
        model_roles: global.model_roles,
        plugins: Vec::new(),
        marp: global.marp,
    }
}

struct LimitThenSuccessProvider {
    prompts: std::sync::Mutex<Vec<String>>,
}

struct ImmediateFailureProvider {
    calls: std::sync::atomic::AtomicUsize,
}

struct StaticPromptCatalog;

impl PromptCatalog for StaticPromptCatalog {
    fn render(
        &self,
        request: PromptRenderRequest,
    ) -> std::result::Result<RenderedPrompt, PromptError> {
        Ok(RenderedPrompt {
            text: format!("rendered {}", request.id),
            provenance: PromptProvenance {
                id: request.id,
                origin: PromptOrigin::Bundled,
                version: 1,
                content_hash: format!("hash-{}", request.id),
            },
        })
    }

    fn validate(&self) -> std::result::Result<Vec<PromptProvenance>, PromptError> {
        Ok(Vec::new())
    }
}

struct RepairProvider {
    calls: std::sync::atomic::AtomicUsize,
    response: String,
}

#[async_trait::async_trait]
impl TextGenerationProvider for RepairProvider {
    async fn generate_text(
        &self,
        request: TextGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> crate::errors::SfumatoResult<TextGenerationResponse> {
        assert!(request.tools.is_empty());
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(TextGenerationResponse {
            text: self.response.clone(),
        })
    }
}

fn theme_package() -> ThemePackage {
    ThemePackage {
        root: PathBuf::from("/tmp/theme"),
        manifest: ThemeManifest {
            schema_version: THEME_SCHEMA_VERSION,
            name: "sfumato-default".to_string(),
            description: "test".to_string(),
            tokens: ThemeTokens::default(),
            adapters: ThemeAdapters {
                marp_css: PathBuf::from("marp/theme.css"),
                html: None,
            },
        },
    }
}

#[async_trait::async_trait]
impl TextGenerationProvider for ImmediateFailureProvider {
    async fn generate_text(
        &self,
        _request: TextGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> crate::errors::SfumatoResult<TextGenerationResponse> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(crate::errors::SfumatoError::provider(
            crate::errors::ErrorClass::Unavailable,
            "connector unavailable",
        ))
    }
}

#[async_trait::async_trait]
impl TextGenerationProvider for LimitThenSuccessProvider {
    async fn generate_text(
        &self,
        request: TextGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> crate::errors::SfumatoResult<TextGenerationResponse> {
        let mut prompts = self.prompts.lock().unwrap();
        prompts.push(request.user_prompt);
        if prompts.len() == 1 {
            return Err(TextGenerationLimitError::output(
                "reviewer".to_string(),
                16_000,
                Some("length".to_string()),
                Some(16_000),
                Some(11_504),
                true,
            )
            .into());
        }
        Ok(TextGenerationResponse {
            text: "compact response".to_string(),
        })
    }
}

#[tokio::test]
async fn token_limit_retries_once_with_the_compact_request() {
    let provider = LimitThenSuccessProvider {
        prompts: std::sync::Mutex::new(Vec::new()),
    };
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let event_log = events.clone();
    let sink = Some(std::sync::Arc::new(move |event| {
        event_log.lock().unwrap().push(event);
    })
        as std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>);

    let outcome = generate_with_compact_retry(
        &provider,
        TextGenerationRequest::new("system".into(), "full payload".into()),
        TextGenerationRequest::new("system".into(), "compact payload".into()),
        GenerationStage::SemanticReview,
        &OperationContext::detached(),
        OperationStage::Review,
        &sink,
    )
    .await
    .unwrap();

    assert_eq!(outcome.response.text, "compact response");
    assert!(
        outcome
            .limit_error
            .unwrap()
            .contains("reasoning tokens: 11504")
    );
    assert_eq!(
        provider.prompts.lock().unwrap().as_slice(),
        ["full payload", "compact payload"]
    );
    assert!(matches!(
        events.lock().unwrap().as_slice(),
        [TextGenerationEvent::ContextCompactionStarted {
            stage: GenerationStage::SemanticReview,
            ..
        }]
    ));
}

#[tokio::test]
async fn unrelated_provider_errors_do_not_trigger_compaction() {
    let provider = ImmediateFailureProvider {
        calls: std::sync::atomic::AtomicUsize::new(0),
    };

    let error = generate_with_compact_retry(
        &provider,
        TextGenerationRequest::new("system".into(), "full payload".into()),
        TextGenerationRequest::new("system".into(), "compact payload".into()),
        GenerationStage::Draft,
        &OperationContext::detached(),
        OperationStage::Draft,
        &None,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("connector unavailable"));
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn invalid_draft_receives_one_dedicated_structural_repair() {
    let provider = RepairProvider {
        calls: std::sync::atomic::AtomicUsize::new(0),
        response: "---\nmarp: true\ntheme: sfumato-default\npaginate: true\nmath: mathjax\n---\n\n# Series de Fourier\n\n---\n\n## Intuicion\n\nUna suma de armonicos."
            .to_string(),
    };
    let config = effective_config();
    let outcome = normalize_and_repair_draft_once(
        &StaticPromptCatalog,
        &provider,
        &config,
        &theme_package(),
        "Explica visualmente las series de Fourier",
        "Ensenar en espanol.",
        "Series de Fourier",
        "# Series de Fourier\n\nNo slide separator",
        &OperationContext::detached(),
        &None,
        "draft-model",
    )
    .await
    .unwrap();

    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    validate_normalized_deck(&outcome.markdown, "Series de Fourier").unwrap();
    assert!(
        outcome
            .prompts
            .iter()
            .any(|prompt| prompt.id == PromptId::SlidesValidationRepairSystem)
    );
    assert!(
        outcome
            .prompts
            .iter()
            .any(|prompt| prompt.id == PromptId::SlidesValidationRepairUser)
    );
}

#[test]
fn strips_markdown_code_fence() {
    let text = "```markdown\n---\nmarp: true\n---\n# Title\n```";
    assert!(strip_code_fence(text).starts_with("---"));
}

#[test]
fn strips_marp_code_fence_without_leaving_the_language_label() {
    let text = "```marp\n---\nmarp: true\n---\n# Title\n```";
    let stripped = strip_code_fence(text);

    assert!(stripped.starts_with("---"));
    assert!(!stripped.starts_with("marp\n"));
}

#[test]
fn strips_unclosed_marp_document_fence() {
    let text = "```marp\n---\nmarp: true\n---\n\n# Demo\n\n---\n\n## Concept";

    let stripped = strip_code_fence(text);

    assert!(stripped.starts_with("---\nmarp: true"));
    assert!(stripped.ends_with("## Concept"));
    assert!(!stripped.contains("```marp"));
}

#[test]
fn preserves_inner_fences_when_outer_marp_fence_is_unclosed() {
    let text = "```marp\n---\nmarp: true\n---\n\n# Demo\n\n---\n\n```mermaid\ngraph LR\nA-->B\n```";

    let stripped = strip_code_fence(text);

    assert!(stripped.contains("```mermaid\ngraph LR\nA-->B\n```"));
}

#[test]
fn normalizes_a_fenced_marp_deck_without_metadata_slides() {
    let mut config = effective_config();
    config.theme = "gruvbox".to_string();
    let generated = "```marp\n---\nmarp: true\ntheme: gruvbox\nmath: mathjax\npaginate: true\n---\n\n<!-- _class: lead -->\n\n# Fourier Series\n\n---\n\n## Intuition\n\nContent.\n```";
    let markdown = normalize_marp_markdown(generated, &config, "Fourier Series").unwrap();

    assert_eq!(markdown.matches("marp: true").count(), 1);
    assert_eq!(markdown.matches("# Fourier Series").count(), 1);
    assert!(!markdown.contains("\n\nmarp\n\n---"));
    assert!(!markdown.contains("\n\nmarp: true\ntheme: gruvbox"));
}

#[test]
fn normalizes_an_unclosed_fenced_marp_deck() {
    let mut config = effective_config();
    config.theme = "gruvbox".to_string();
    let generated =
        "```marp\n---\nmarp: true\n---\n\n# Fourier Series\n\n---\n\n## Intuition\n\nContent.";

    let markdown = normalize_marp_markdown(generated, &config, "Fourier Series").unwrap();

    validate_normalized_deck(&markdown, "Fourier Series").unwrap();
    assert!(!markdown.contains("```marp"));
    assert!(markdown.contains("## Intuition"));
}

#[test]
fn normalizes_unclosed_document_wrapper_without_consuming_mermaid_fence() {
    let config = effective_config();
    let generated = "```marp\n---\nmarp: true\n---\n\n# Demo\n\n---\n\n## Flow\n\n```mermaid\ngraph LR\nA-->B\n```";

    let markdown = normalize_marp_markdown(generated, &config, "Demo").unwrap();
    let diagrams = extract_mermaid_blocks(&markdown).unwrap();

    validate_normalized_deck(&markdown, "Demo").unwrap();
    assert_eq!(diagrams.len(), 1);
    assert_eq!(diagrams[0].source, "graph LR\nA-->B");
}

#[test]
fn closes_unclosed_mermaid_fences_at_slide_boundaries() {
    let normalized = normalize_marp_markdown(
        "# Fourier\n\n---\n\n## Diagram\n\n```mermaid\ngraph LR\nA --> B\n\n---\n\n## Next\n\nText",
        &effective_config(),
        "Fourier",
    )
    .unwrap();

    let blocks = extract_mermaid_blocks(&normalized).unwrap();
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].source.ends_with("A --> B"));
    let document = SlideDeckDocument::from_marp(&normalized, "Fourier").unwrap();
    assert_eq!(document.slide_count(), 3);
}

#[test]
fn applies_focused_mermaid_repair_as_revision_guarded_json_patch() {
    let markdown = normalize_marp_markdown(
        "# Fourier\n\n---\n\n## Diagram\n\n```mermaid\ngraph LR\nA -- > B\n```",
        &effective_config(),
        "Fourier",
    )
    .unwrap();
    let document = SlideDeckDocument::from_marp(&markdown, "Fourier").unwrap();
    let snapshot = document.snapshot().unwrap();
    let deck_revision = snapshot.document["revision"].clone();
    let slide_id = snapshot.document["order"][1].as_str().unwrap();
    let slide_revision = snapshot.document["slides"][slide_id]["revision"].clone();
    let repaired = "## Diagram\n\n```mermaid\ngraph LR\nA --> B\n```";
    let patch = serde_json::json!([
        {"op": "test", "path": "/revision", "value": deck_revision},
        {
            "op": "test",
            "path": format!("/slides/{slide_id}/revision"),
            "value": slide_revision
        },
        {
            "op": "replace",
            "path": format!("/slides/{slide_id}/markdown"),
            "value": repaired
        }
    ]);

    let candidate = apply_mermaid_repair_response(
        &markdown,
        "Fourier",
        &patch.to_string(),
        &effective_config(),
    )
    .unwrap();

    assert!(candidate.contains("A --> B"));
    assert!(!candidate.contains("A -- > B"));
    assert!(candidate.contains("# Fourier"));
}

#[test]
fn rejects_invalid_normalized_deck_before_rendering() {
    let markdown = "---\nmarp: true\n---\n\n# Demo\n\n---\n\n```rust\nfn main() {}";

    let error = validate_normalized_deck(markdown, "Demo").unwrap_err();

    assert!(error.to_string().contains("invalid after normalization"));
}

#[test]
fn rejects_paths_outside_artifact_root() {
    assert!(ensure_inside(Path::new("/tmp/out"), Path::new("/tmp/elsewhere/a.md")).is_err());
}

#[test]
fn normalizes_frontmatter() {
    let mut config = effective_config();
    config.theme = "gruvbox".to_string();
    let markdown = normalize_marp_markdown("# Demo\n\n---\n\n## One", &config, "Demo").unwrap();
    assert!(markdown.contains("marp: true"));
    assert!(markdown.contains("theme: gruvbox"));
    assert!(markdown.contains("math: mathjax"));
}

#[test]
fn normalizes_existing_math_frontmatter_to_mathjax() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\nmath: katex\n---\n\n# Demo\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.contains("math: mathjax"));
    assert!(!markdown.contains("math: katex"));
}

#[test]
fn promotes_late_marp_frontmatter_and_preserves_title_slide() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\n\n# Demo\n\n---\nmarp: true\npaginate: true\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.starts_with("---\n"));
    assert!(markdown.contains("marp: true"));
    assert!(markdown.contains("\n\n# Demo\n\n---\n\n## One"));
    assert!(!markdown.starts_with("---\n\n# Demo\n\n---\nmarp: true"));
}

#[test]
fn inserts_missing_title_after_canonical_frontmatter() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n## One\n\n---\n\n## Two",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.starts_with(
        "---\nmarp: true\ntheme: sfumato-default\npaginate: true\nmath: mathjax\n---\n\n# Demo\n\n---\n\n## One"
    ));
    assert!(!markdown.starts_with("---\n\n# Demo\n\n---\nmarp: true"));
}

#[test]
fn removes_generated_frontmatter_css() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\nstyle: |\n  section { color: red; }\n  h1 { color: blue; }\nmath: katex\ncustom: ignored\n---\n\n# Demo\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.starts_with(
        "---\nmarp: true\ntheme: sfumato-default\npaginate: true\nmath: mathjax\n---"
    ));
    assert!(!markdown.contains("style:"));
    assert!(!markdown.contains("section { color: red; }"));
    assert!(!markdown.contains("custom: ignored"));
    assert!(markdown.contains("math: mathjax"));
    assert!(!markdown.contains("math: katex"));
}

#[test]
fn removes_generated_css_blocks_from_slide_body() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n<style>\nsection { color: red; }\n</style>\n\n```css\n@theme bad\nsection { color: blue; }\n```\n\n# Demo\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(!markdown.contains("<style"));
    assert!(!markdown.contains("@theme bad"));
    assert!(!markdown.contains("color: blue"));
    assert!(markdown.contains("# Demo"));
}

#[test]
fn removes_duplicate_leading_title_only_slides() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n# Demo\n\n---\n\n<!-- _class: title -->\n\n# Demo\n\n## Real title slide\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(!markdown.contains("# Demo\n\n---\n\n<!-- _class: title -->"));
    assert!(markdown.contains("<!-- _class: title -->\n\n# Demo"));
}

#[test]
fn removes_raw_svg_blocks_from_generated_decks() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "# Demo\n\n---\n\n<svg width=\"100\"><path d=\"M0 0\" /></svg>\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(!markdown.to_lowercase().contains("<svg"));
    assert!(!markdown.to_lowercase().contains("<path"));
}

#[test]
fn removes_common_html_wrapper_tags_from_generated_decks() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "# Demo\n\n---\n\n<section><div>Key idea</div></section>",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.contains("Key idea"));
    assert!(!markdown.to_lowercase().contains("<section"));
    assert!(!markdown.to_lowercase().contains("<div"));
}

#[test]
fn extracts_mermaid_fences_for_pre_rendering() {
    let markdown = "# Demo\n\n```mermaid\ngraph TD\n  A-->B\n```\n\nAfter";
    let blocks = extract_mermaid_blocks(markdown).unwrap();

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].source, "graph TD\n  A-->B");
    assert_eq!(
        &markdown[blocks[0].start..blocks[0].end],
        "```mermaid\ngraph TD\n  A-->B\n```"
    );
}

#[test]
fn rendered_mermaid_images_are_bounded_for_marp_layout() {
    assert_eq!(
        mermaid_image_markdown("diagram-deadbeef"),
        "![height:300px](diagrams/diagram-deadbeef.svg)"
    );
}

#[test]
fn normalizes_mermaid_labels_before_rendering() {
    let source = "flowchart TB\n    A[\"a₀/2average heightDC offset\"]";
    let normalized = normalize_mermaid_source(source);

    assert!(normalized.contains("a₀/2 average height DC<br/>offset"));
}

#[test]
fn preserves_existing_mermaid_label_breaks() {
    let source = "flowchart TB\n    A[\"first line\\nsecond line\"]";
    let normalized = normalize_mermaid_source(source);

    assert!(normalized.contains("first line<br/>second line"));
}

#[test]
fn maps_theme_tokens_to_mermaid_theme_variables() {
    let config = mermaid_theme_config(&crate::themes::ThemeTokens {
        colors: BTreeMap::from([
            ("background".to_string(), "#fbf1c7".to_string()),
            ("surface".to_string(), "#f9f5d7".to_string()),
            ("surface-alt".to_string(), "#ebdbb2".to_string()),
            ("text".to_string(), "#3c3836".to_string()),
            ("primary".to_string(), "#9d0006".to_string()),
            ("accent".to_string(), "#af3a03".to_string()),
        ]),
        fonts: BTreeMap::from([("body".to_string(), "Inter, sans-serif".to_string())]),
    });
    let rendered = serde_json::to_string(&config).unwrap();

    assert!(rendered.contains("\"theme\":\"base\""));
    assert!(rendered.contains("\"primaryBorderColor\":\"#9d0006\""));
    assert!(rendered.contains("\"lineColor\":\"#af3a03\""));
    assert!(rendered.contains("\"fontFamily\":\"Inter, sans-serif\""));
}

#[test]
fn summarizes_declared_tools_for_generation_output() {
    let tool = ToolDefinition {
        kind: "function".to_string(),
        function: crate::providers::ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({ "type": "object" }),
        },
    };

    let summaries = summarize_tools(&[tool]);

    assert_eq!(summaries[0].name, "sfumato_read_file");
    assert_eq!(summaries[0].description, "Read a file");
}

#[test]
fn constrains_unsized_generated_images_for_marp() {
    let markdown = "## Visual\n\n![Fourier illustration](images/generated-fourier.png)";

    assert_eq!(
        constrain_generated_images(markdown),
        "## Visual\n\n![height:420px](images/generated-fourier.png)"
    );
}

#[test]
fn preserves_explicit_generated_image_dimensions() {
    let markdown = "![height:320px](images/generated-fourier.png)";

    assert_eq!(constrain_generated_images(markdown), markdown);
}

#[test]
fn preserves_generated_background_image_layout() {
    let markdown = "![bg contain](images/generated-fourier.png)";

    assert_eq!(constrain_generated_images(markdown), markdown);
}

#[test]
fn does_not_constrain_unmanaged_images() {
    let markdown = "![Architecture](../course-assets/architecture.png)";

    assert_eq!(constrain_generated_images(markdown), markdown);
}

#[test]
fn parses_a_focused_title_repair_response() {
    let title = parse_repaired_title(
        "# From Periodic Signals to Harmonic Spectra\n",
        "Explain Fourier series visually",
    )
    .unwrap();

    assert_eq!(title, "From Periodic Signals to Harmonic Spectra");
}

#[test]
fn title_repair_rejects_the_original_instruction() {
    assert!(
        parse_repaired_title(
            "Explain Fourier series visually",
            "Explain Fourier series visually"
        )
        .is_err()
    );
}

#[test]
fn extracts_the_drafters_title_from_the_first_h1() {
    let generated = "---\nmarp: true\ntheme: demo\n---\n\n<!-- _class: lead -->\n\n# **Fourier Series: From Waves to Spectra**\n\n---\n\n## Intuition";

    assert_eq!(
        extract_generated_title(generated).as_deref(),
        Some("Fourier Series: From Waves to Spectra")
    );
}

#[test]
fn title_extraction_ignores_h1_examples_inside_code_fences() {
    let generated = "---\nmarp: true\n---\n\n```markdown\n# Not the deck title\n```\n\n# The Actual Deck Title\n\n---\n\n## One";

    assert_eq!(
        extract_generated_title(generated).as_deref(),
        Some("The Actual Deck Title")
    );
}

#[test]
fn title_extraction_rejects_a_deck_without_an_h1() {
    let generated = "---\nmarp: true\n---\n\n## First topic\n\n---\n\n## Second topic";

    assert_eq!(extract_generated_title(generated), None);
}

#[test]
fn recognizes_when_the_drafter_reuses_the_instruction_as_title() {
    assert!(titles_are_equivalent(
        "Explain Fourier Series!",
        "explain fourier series"
    ));
    assert!(!titles_are_equivalent(
        "Fourier Series: From Waves to Spectra",
        "Explain Fourier series visually"
    ));
}

#[test]
fn normalization_keeps_the_artifact_title_on_the_first_slide() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n# Reviewer Changed It\n\n---\n\n## One",
        &config,
        "Fourier Series: A Visual Guide",
    )
    .unwrap();

    assert!(markdown.contains("\n\n# Fourier Series: A Visual Guide\n\n---"));
    assert!(!markdown.contains("Reviewer Changed It"));
}

#[test]
fn artifact_paths_use_the_generated_title_slug() {
    let (markdown, pdf) =
        slide_artifact_paths(Path::new("/tmp/slides"), "Fourier Series: A Visual Guide").unwrap();

    assert_eq!(
        markdown,
        PathBuf::from("/tmp/slides/fourier-series-a-visual-guide.md")
    );
    assert_eq!(
        pdf,
        PathBuf::from("/tmp/slides/fourier-series-a-visual-guide.pdf")
    );
}

#[test]
fn compact_source_bundle_distributes_a_fixed_budget_across_files() {
    let documents = (1..=4)
        .map(|index| SourceDocument {
            path: PathBuf::from(format!("/tmp/source-{index}.md")),
            content: format!("# Source {index}\n{}", "evidence ".repeat(2_000)),
        })
        .collect::<Vec<_>>();

    let compact = build_compact_source_bundle(&documents, 4_000);

    assert!(compact.chars().count() <= 4_030);
    for index in 1..=4 {
        assert!(compact.contains(&format!("source-{index}.md")));
    }
    assert!(compact.contains("[...truncated by sfumato...]"));
}

#[test]
fn model_output_retry_excludes_missing_dependencies() {
    assert!(should_retry_model_output(&SfumatoError::new(
        ErrorCode::Validation,
        ErrorClass::InvalidOutput,
        "Mermaid CLI exited with a parse error",
    )));
    assert!(!should_retry_model_output(&SfumatoError::render(
        ErrorClass::Unavailable,
        "Mermaid CLI is not installed",
    )));
}

#[test]
fn locates_slide_ranges_without_counting_separators_inside_code_fences() {
    let markdown = "---\nmarp: true\ntheme: demo\n---\n\n# One\n\n---\n\n## Two\n\n```yaml\n---\nvalue: true\n---\n```\n\n---\n\n## Three";
    let ranges = slide_ranges(markdown).unwrap();

    assert_eq!(ranges.len(), 3);
    assert_eq!(markdown[ranges[0].start..ranges[0].end].trim(), "# One");
    assert!(markdown[ranges[1].start..ranges[1].end].contains("```yaml"));
    assert_eq!(markdown[ranges[2].start..ranges[2].end].trim(), "## Three");
}

#[test]
fn normalizes_reviewer_fragment_without_document_frontmatter() {
    let generated = "```markdown\n---\nmarp: true\ntheme: wrong\n---\n\n---\n\n## Fixed\n\nShort content.\n\n---\n```";
    let fragment = normalize_slide_replacement(generated).unwrap();

    assert_eq!(fragment, "## Fixed\n\nShort content.");
    assert!(!fragment.contains("marp: true"));
}

#[test]
fn layout_score_prioritizes_issue_count_then_overflow() {
    let one_issue = vec![SlideLayoutIssue {
        slide: 1,
        title: "One".to_string(),
        vertical_overflow_px: 100,
        horizontal_overflow_px: 0,
    }];
    let two_issues = vec![
        SlideLayoutIssue {
            slide: 1,
            title: "One".to_string(),
            vertical_overflow_px: 1,
            horizontal_overflow_px: 0,
        },
        SlideLayoutIssue {
            slide: 2,
            title: "Two".to_string(),
            vertical_overflow_px: 1,
            horizontal_overflow_px: 0,
        },
    ];

    assert!(layout_score(&one_issue) < layout_score(&two_issues));
}

#[test]
fn model_tool_rounds_uses_profile_option_or_default() {
    let mut profile = crate::config::ModelProfile {
        connector: "openrouter".to_string(),
        model: "model".to_string(),
        capabilities: vec![crate::config::Capability::Text],
        options: Default::default(),
    };

    assert_eq!(model_tool_rounds(&profile), 8);

    profile.options.text.max_tool_rounds = Some(12);

    assert_eq!(model_tool_rounds(&profile), 12);
}

#[test]
fn review_summary_reports_context_compaction_for_json_callers() {
    let summary = SlideReviewSummary::enabled();
    let json = serde_json::to_value(summary).unwrap();

    assert_eq!(json["context_compaction"], "not_needed");
}
