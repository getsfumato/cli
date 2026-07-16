//! Typed prompt contexts and request builders for slide workflows.

use super::*;

pub(super) const DRAFT_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesDraftSystem,
    user: PromptId::SlidesDraftUser,
    tool_exhausted: PromptId::SlidesDraftToolExhaustedUser,
};
pub(super) const COMPACT_DRAFT_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesCompactDraftSystem,
    user: PromptId::SlidesCompactDraftUser,
    tool_exhausted: PromptId::SlidesDraftToolExhaustedUser,
};
pub(super) const TITLE_REPAIR_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesTitleRepairSystem,
    user: PromptId::SlidesTitleRepairUser,
    tool_exhausted: PromptId::SlidesDraftToolExhaustedUser,
};
pub(super) const REVIEW_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesReviewSystem,
    user: PromptId::SlidesReviewUser,
    tool_exhausted: PromptId::SlidesReviewToolExhaustedUser,
};
pub(super) const COMPACT_REVIEW_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesCompactReviewSystem,
    user: PromptId::SlidesCompactReviewUser,
    tool_exhausted: PromptId::SlidesReviewToolExhaustedUser,
};
pub(super) const MERMAID_REPAIR_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesMermaidRepairSystem,
    user: PromptId::SlidesMermaidRepairUser,
    tool_exhausted: PromptId::SlidesReviewToolExhaustedUser,
};
pub(super) const LAYOUT_REPAIR_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesLayoutRepairSystem,
    user: PromptId::SlidesLayoutRepairUser,
    tool_exhausted: PromptId::SlidesLayoutRepairToolExhaustedUser,
};
pub(super) const COMPACT_LAYOUT_REPAIR_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesCompactLayoutRepairSystem,
    user: PromptId::SlidesCompactLayoutRepairUser,
    tool_exhausted: PromptId::SlidesLayoutRepairToolExhaustedUser,
};

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct SlidePromptContext {
    pub(super) learning_style: String,
    pub(super) project: String,
    pub(super) project_root: String,
    pub(super) theme_name: String,
    pub(super) theme_colors: String,
    pub(super) theme_fonts: String,
    pub(super) instruction: String,
    pub(super) project_instructions: String,
    pub(super) title: String,
    pub(super) title_provided: bool,
    pub(super) image_generation_available: bool,
    pub(super) source_bundle: String,
    pub(super) tools: Vec<GenerationToolSummary>,
    pub(super) deck_snapshot: String,
    pub(super) diagram_error: String,
    pub(super) validation_error: String,
    pub(super) headings: String,
    pub(super) retry_present: bool,
    pub(super) retry_error: String,
    pub(super) retry_invalid_response: String,
    pub(super) issue_report: String,
    pub(super) slide_markdown: String,
    pub(super) compact: bool,
    pub(super) max_tool_rounds: usize,
}

pub(super) fn render_prompt_request(
    catalog: &dyn PromptCatalog,
    pair: PromptPair,
    context: &SlidePromptContext,
) -> Result<TextGenerationRequest> {
    let variables = PromptVariables::from_serializable(context)?;
    let system = catalog.render(PromptRenderRequest {
        id: pair.system,
        variables: variables.clone(),
    })?;
    let user = catalog.render(PromptRenderRequest {
        id: pair.user,
        variables: variables.clone(),
    })?;
    let exhausted = catalog.render(PromptRenderRequest {
        id: pair.tool_exhausted,
        variables,
    })?;
    let mut request = TextGenerationRequest::new(system.text, user.text);
    request.tool_exhausted_prompt = Some(exhausted.text);
    request.prompt_provenance = vec![system.provenance, user.provenance, exhausted.provenance];
    request.max_tool_rounds = context.max_tool_rounds;
    Ok(request)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn review_prompt_context(
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    project_instructions: String,
    source_bundle: String,
    deck_snapshot: String,
    retry: Option<&ReviewRetryContext>,
    compact: bool,
    max_tool_rounds: usize,
) -> SlidePromptContext {
    SlidePromptContext {
        project: config.project_name.clone(),
        project_root: config.project_root.display().to_string(),
        theme_name: theme.manifest.name.clone(),
        theme_colors: format_tokens(&theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&theme.manifest.tokens.fonts),
        instruction: instruction.to_string(),
        project_instructions,
        source_bundle,
        deck_snapshot,
        retry_present: retry.is_some(),
        retry_error: retry
            .map(|retry| excerpt(&retry.error, if compact { 1_000 } else { 2_000 }))
            .unwrap_or_default(),
        retry_invalid_response: retry
            .map(|retry| {
                excerpt(
                    &retry.invalid_response,
                    if compact { 4_000 } else { 12_000 },
                )
            })
            .unwrap_or_default(),
        compact,
        max_tool_rounds,
        ..Default::default()
    }
}

pub(super) struct DraftPromptRequestContext<'a> {
    pub(super) catalog: &'a dyn PromptCatalog,
    pub(super) config: &'a EffectiveConfig,
    pub(super) theme: &'a ThemePackage,
    pub(super) instruction: &'a str,
    pub(super) title: Option<&'a str>,
    pub(super) source_bundle: &'a str,
    pub(super) image_generation_available: bool,
    pub(super) project_instructions: &'a str,
    pub(super) tools: &'a [GenerationToolSummary],
    pub(super) max_tool_rounds: usize,
    pub(super) event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

pub(super) struct ReviewPromptRequestContext<'a> {
    pub(super) catalog: &'a dyn PromptCatalog,
    pub(super) config: &'a EffectiveConfig,
    pub(super) theme: &'a ThemePackage,
    pub(super) instruction: &'a str,
    pub(super) source_bundle: &'a str,
    pub(super) snapshot: &'a ReviewSnapshot,
    pub(super) retry: Option<&'a ReviewRetryContext>,
    pub(super) project_instructions: &'a str,
    pub(super) max_tool_rounds: usize,
}

pub(super) fn build_generation_request(
    args: DraftPromptRequestContext<'_>,
) -> Result<TextGenerationRequest> {
    let learning_style = if args.config.user.learning_style.is_empty() {
        "not specified".to_string()
    } else {
        args.config.user.learning_style.join(", ")
    };
    let context = SlidePromptContext {
        learning_style,
        project: args.config.project_name.clone(),
        project_root: args.config.project_root.display().to_string(),
        theme_name: args.theme.manifest.name.clone(),
        theme_colors: format_tokens(&args.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&args.theme.manifest.tokens.fonts),
        instruction: args.instruction.to_string(),
        project_instructions: args.project_instructions.to_string(),
        title: args.title.unwrap_or_default().to_string(),
        title_provided: args.title.is_some(),
        image_generation_available: args.image_generation_available,
        source_bundle: args.source_bundle.to_string(),
        tools: args.tools.to_vec(),
        max_tool_rounds: args.max_tool_rounds,
        ..Default::default()
    };
    render_prompt_request(args.catalog, DRAFT_PROMPTS, &context)
}

pub(super) fn build_compact_generation_request(
    args: DraftPromptRequestContext<'_>,
) -> Result<TextGenerationRequest> {
    let learning_style = if args.config.user.learning_style.is_empty() {
        "not specified".to_string()
    } else {
        args.config.user.learning_style.join(", ")
    };
    let context = SlidePromptContext {
        learning_style,
        project: args.config.project_name.clone(),
        project_root: args.config.project_root.display().to_string(),
        theme_name: args.theme.manifest.name.clone(),
        theme_colors: format_tokens(&args.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&args.theme.manifest.tokens.fonts),
        instruction: args.instruction.to_string(),
        project_instructions: excerpt(args.project_instructions, 4_000),
        title: args.title.unwrap_or_default().to_string(),
        title_provided: args.title.is_some(),
        source_bundle: args.source_bundle.to_string(),
        compact: true,
        max_tool_rounds: args.max_tool_rounds,
        ..Default::default()
    };
    let mut request = render_prompt_request(args.catalog, COMPACT_DRAFT_PROMPTS, &context)?;
    request.event_sink = args.event_sink;
    Ok(request)
}

pub(super) fn build_title_repair_request(
    catalog: &dyn PromptCatalog,
    config: &EffectiveConfig,
    instruction: &str,
    draft: &str,
    validation_error: &str,
    project_instructions: &str,
) -> Result<TextGenerationRequest> {
    let project_instructions = excerpt(project_instructions, 1_500);
    let headings = markdown_headings(draft);
    let headings = if headings.is_empty() {
        "No usable headings were found in the draft.".to_string()
    } else {
        headings
            .into_iter()
            .take(20)
            .map(|heading| format!("- {heading}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let context = SlidePromptContext {
        project: config.project_name.clone(),
        instruction: instruction.to_string(),
        validation_error: validation_error.to_string(),
        project_instructions,
        headings,
        max_tool_rounds: 0,
        ..Default::default()
    };
    render_prompt_request(catalog, TITLE_REPAIR_PROMPTS, &context)
}

pub(super) fn build_review_request(
    args: ReviewPromptRequestContext<'_>,
) -> Result<TextGenerationRequest> {
    let snapshot = serde_json::to_string_pretty(args.snapshot)
        .context("Could not serialize slide deck review snapshot")?;
    let context = review_prompt_context(
        args.config,
        args.theme,
        args.instruction,
        args.project_instructions.to_string(),
        args.source_bundle.to_string(),
        snapshot,
        args.retry,
        false,
        args.max_tool_rounds,
    );
    render_prompt_request(args.catalog, REVIEW_PROMPTS, &context)
}

pub(super) fn build_compact_review_request(
    args: ReviewPromptRequestContext<'_>,
) -> Result<TextGenerationRequest> {
    let snapshot = compact_review_snapshot(args.snapshot)?;
    let context = review_prompt_context(
        args.config,
        args.theme,
        args.instruction,
        excerpt(args.project_instructions, 4_000),
        args.source_bundle.to_string(),
        snapshot,
        args.retry,
        true,
        args.max_tool_rounds,
    );
    render_prompt_request(args.catalog, COMPACT_REVIEW_PROMPTS, &context)
}

pub(super) fn build_mermaid_repair_request(
    catalog: &dyn PromptCatalog,
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    project_instructions: &str,
    snapshot: &ReviewSnapshot,
    diagram_error: &str,
) -> Result<TextGenerationRequest> {
    let context = SlidePromptContext {
        project: config.project_name.clone(),
        instruction: instruction.to_string(),
        theme_name: theme.manifest.name.clone(),
        project_instructions: excerpt(project_instructions, 4_000),
        deck_snapshot: serde_json::to_string_pretty(snapshot)
            .context("Could not serialize Mermaid repair snapshot")?,
        diagram_error: excerpt(diagram_error, 6_000),
        max_tool_rounds: 0,
        ..Default::default()
    };
    render_prompt_request(catalog, MERMAID_REPAIR_PROMPTS, &context)
}

pub(super) fn compact_review_snapshot(snapshot: &ReviewSnapshot) -> Result<String> {
    let document = snapshot
        .document
        .as_object()
        .context("Slide review snapshot document must be an object")?;
    let slides = document
        .get("slides")
        .and_then(serde_json::Value::as_object)
        .context("Slide review snapshot is missing slides")?;
    let order = document
        .get("order")
        .and_then(serde_json::Value::as_array)
        .context("Slide review snapshot is missing slide order")?;
    let per_slide_budget = (30_000 / slides.len().max(1)).clamp(500, 1_800);
    let compact_slides = order
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|id| {
            slides
                .get(id)
                .and_then(serde_json::Value::as_object)
                .map(|slide| (id, slide))
        })
        .map(|(id, slide)| {
            let markdown = slide
                .get("markdown")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            serde_json::json!({
                "id": id,
                "revision": slide.get("revision"),
                "kind": slide.get("kind"),
                "heading": slide.get("heading"),
                "elements": slide.get("elements"),
                "markdown": excerpt(markdown, per_slide_budget),
                "markdown_complete": markdown.chars().count() <= per_slide_budget,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": snapshot.schema_version,
        "format": snapshot.format,
        "revision": snapshot.revision,
        "title": document.get("title"),
        "order": order,
        "slides": compact_slides,
    }))
    .context("Could not serialize compact slide review snapshot")
}

pub(super) struct ReviewRetryContext {
    pub(super) invalid_response: String,
    pub(super) error: String,
}

pub(super) fn build_layout_repair_request(
    catalog: &dyn PromptCatalog,
    context: LayoutRepairRequestContext<'_>,
    retry: Option<&LayoutRepairRetryContext>,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
) -> Result<TextGenerationRequest> {
    let issue_report =
        serde_json::to_string_pretty(context.issue).unwrap_or_else(|_| "{}".to_string());
    let prompt_context = SlidePromptContext {
        project: context.config.project_name.clone(),
        instruction: context.instruction.to_string(),
        title: context.title.to_string(),
        title_provided: true,
        theme_name: context.theme.manifest.name.clone(),
        theme_colors: format_tokens(&context.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&context.theme.manifest.tokens.fonts),
        project_instructions: context.project_instructions.to_string(),
        issue_report,
        slide_markdown: context.slide_markdown.to_string(),
        retry_present: retry.is_some(),
        retry_error: retry
            .map(|retry| excerpt(&retry.error, 4_000))
            .unwrap_or_default(),
        retry_invalid_response: retry
            .map(|retry| excerpt(&retry.invalid_response, 12_000))
            .unwrap_or_default(),
        max_tool_rounds: 0,
        ..Default::default()
    };
    let mut request = render_prompt_request(catalog, LAYOUT_REPAIR_PROMPTS, &prompt_context)?;
    request.event_sink = event_sink;
    Ok(request)
}

pub(super) fn build_compact_layout_repair_request(
    catalog: &dyn PromptCatalog,
    context: LayoutRepairRequestContext<'_>,
    retry: Option<&LayoutRepairRetryContext>,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
) -> Result<TextGenerationRequest> {
    let compact_retry = retry.map(|retry| LayoutRepairRetryContext {
        invalid_response: excerpt(&retry.invalid_response, 2_000),
        error: excerpt(&retry.error, 1_000),
    });
    let compact_instructions = excerpt(context.project_instructions, 1_500);
    let issue_report =
        serde_json::to_string_pretty(context.issue).unwrap_or_else(|_| "{}".to_string());
    let prompt_context = SlidePromptContext {
        project: context.config.project_name.clone(),
        instruction: context.instruction.to_string(),
        title: context.title.to_string(),
        title_provided: true,
        theme_name: context.theme.manifest.name.clone(),
        theme_colors: format_tokens(&context.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&context.theme.manifest.tokens.fonts),
        project_instructions: compact_instructions,
        issue_report,
        slide_markdown: context.slide_markdown.to_string(),
        retry_present: compact_retry.is_some(),
        retry_error: compact_retry
            .as_ref()
            .map(|retry| retry.error.clone())
            .unwrap_or_default(),
        retry_invalid_response: compact_retry
            .as_ref()
            .map(|retry| retry.invalid_response.clone())
            .unwrap_or_default(),
        compact: true,
        max_tool_rounds: 0,
        ..Default::default()
    };
    let mut request =
        render_prompt_request(catalog, COMPACT_LAYOUT_REPAIR_PROMPTS, &prompt_context)?;
    request.event_sink = event_sink;
    Ok(request)
}

pub(super) struct LayoutRepairRequestContext<'a> {
    pub(super) config: &'a EffectiveConfig,
    pub(super) theme: &'a ThemePackage,
    pub(super) instruction: &'a str,
    pub(super) title: &'a str,
    pub(super) slide_markdown: &'a str,
    pub(super) issue: &'a SlideLayoutIssue,
    pub(super) project_instructions: &'a str,
}

pub(super) struct LayoutRepairRetryContext {
    pub(super) invalid_response: String,
    pub(super) error: String,
}
