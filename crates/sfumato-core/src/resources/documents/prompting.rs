//! Prompt rendering, parsing, and inspection helpers for documents.

use std::path::{Path, PathBuf};

use sfumato_domain::SectionedDocument;

use super::*;

/// Renders one system and user prompt pair from the shared context.
pub(super) fn render_pair(
    catalog: &dyn PromptCatalog,
    system: PromptId,
    user: PromptId,
    context: &DocumentPromptContext,
) -> Result<TextGenerationRequest> {
    let variables = PromptVariables::from_serializable(context)?;
    let system = catalog.render(PromptRenderRequest {
        id: system,
        variables: variables.clone(),
    })?;
    let user = catalog.render(PromptRenderRequest {
        id: user,
        variables: variables.clone(),
    })?;
    let exhausted = catalog.render(PromptRenderRequest {
        id: PromptId::DocumentToolExhaustedUser,
        variables,
    })?;
    let mut request = TextGenerationRequest::new(system.text, user.text);
    request.max_tool_rounds = context.max_tool_rounds;
    request.tool_exhausted_prompt = Some(exhausted.text);
    request.prompt_provenance = vec![system.provenance, user.provenance, exhausted.provenance];
    Ok(request)
}

pub(super) struct CompactRetryOutcome {
    pub(super) response: crate::providers::TextGenerationResponse,
    pub(super) limit_error: Option<String>,
}

/// Runs a request, retrying once with a compacted prompt on a context limit.
pub(super) async fn generate_with_compact_retry(
    provider: &dyn TextGenerationProvider,
    request: TextGenerationRequest,
    compact_request: TextGenerationRequest,
    operation: &OperationContext,
    stage: OperationStage,
) -> Result<CompactRetryOutcome> {
    match provider.generate_text(request, operation, stage).await {
        Ok(response) => Ok(CompactRetryOutcome {
            response,
            limit_error: None,
        }),
        Err(error) if is_context_limit(&error) => {
            let limit_error = format!("{error:#}");
            let response = provider
                .generate_text(compact_request, operation, stage)
                .await
                .map_err(|error| error.context("Model request failed after compacting context"))?;
            Ok(CompactRetryOutcome {
                response,
                limit_error: Some(limit_error),
            })
        }
        Err(error) => Err(error),
    }
}

fn is_context_limit(error: &SfumatoError) -> bool {
    error.class == ErrorClass::Unavailable && error.message.to_ascii_lowercase().contains("context")
        || error.code == ErrorCode::Provider
            && error.message.to_ascii_lowercase().contains("too long")
}

/// Parses drafted Markdown into a validated document.
pub(super) fn parse_document(markdown: &str, title: Option<&str>) -> Result<SectionedDocument> {
    let document = SectionedDocument::from_markdown(markdown).map_err(domain_error)?;
    if let Some(title) = title
        && document.title() != title
    {
        return Err(SfumatoError::render(
            ErrorClass::InvalidOutput,
            format!(
                "The document title is '{}' but '{title}' was requested",
                document.title()
            ),
        ));
    }
    Ok(document)
}

/// Applies a reviewer patch to a copy of the document.
pub(super) fn apply_review(
    document: &SectionedDocument,
    response: &str,
) -> Result<SectionedDocument> {
    let patch = parse_json_patch(response).map_err(domain_error)?;
    let mut candidate = document.clone();
    candidate.apply_patch(&patch).map_err(domain_error)?;
    Ok(candidate)
}

pub(super) fn snapshot_json(document: &SectionedDocument) -> Result<String> {
    Ok(serde_json::to_string_pretty(
        &document.snapshot().map_err(domain_error)?,
    )?)
}

/// Renders every Mermaid fence into a throwaway workspace to prove it compiles.
pub(super) async fn validate_mermaid(
    document: &SectionedDocument,
    theme: &ThemePackage,
    renderer: &dyn DiagramRenderer,
    browser_path: Option<&Path>,
    workspace: &dyn WorkspaceFileSystem,
    operation: &OperationContext,
) -> Result<()> {
    let markdown = document.render().map_err(domain_error)?;
    if extract_mermaid_blocks(&markdown)?.is_empty() {
        return Ok(());
    }
    let temp = workspace.temporary_directory("sfumato-document-mermaid-")?;
    render_mermaid_diagrams(MermaidRenderRequest {
        markdown: &markdown,
        diagrams_dir: &temp.path().join("diagrams"),
        theme,
        renderer,
        browser_path,
        workspace,
        operation,
        stage: OperationStage::Repair,
        image_markdown: document_image_markdown,
    })
    .await
    .map(|_| ())
}

pub(super) struct InspectionContext<'a> {
    pub(super) theme: &'a ThemePackage,
    pub(super) setup: DocumentPageSetup,
    pub(super) project: &'a str,
    pub(super) revision_date: &'a str,
    pub(super) assembler: &'a dyn DocumentAssembler,
    pub(super) renderer: &'a dyn DocumentRenderer,
    pub(super) diagram_renderer: &'a dyn DiagramRenderer,
    pub(super) browser_path: Option<&'a Path>,
    pub(super) workspace: &'a dyn WorkspaceFileSystem,
    pub(super) prepared_assets: &'a crate::resources::project_assets::PreparedProjectAssets,
    pub(super) generated_assets: &'a [PathBuf],
    pub(super) operation: &'a OperationContext,
}

/// Assembles and paginates a candidate to measure its printed pages.
///
/// Runs in a throwaway workspace so a candidate that is never adopted leaves
/// nothing behind in the revision.
pub(super) async fn inspect_document(
    document: &SectionedDocument,
    context: InspectionContext<'_>,
) -> Result<Vec<DocumentFormatIssue>> {
    context
        .operation
        .checkpoint(OperationStage::InspectLayout)?;
    let temp = context
        .workspace
        .temporary_directory("sfumato-document-format-")?;
    let root = temp.path();
    let markdown = document.render().map_err(domain_error)?;
    let (rendered, diagram_artifacts) = render_mermaid_diagrams(MermaidRenderRequest {
        markdown: &markdown,
        diagrams_dir: &root.join("diagrams"),
        theme: context.theme,
        renderer: context.diagram_renderer,
        browser_path: context.browser_path,
        workspace: context.workspace,
        operation: context.operation,
        stage: OperationStage::InspectLayout,
        image_markdown: document_image_markdown,
    })
    .await?;
    context
        .prepared_assets
        .stage_referenced(&rendered, root, context.workspace)?;
    crate::resources::project_assets::stage_referenced_generated_assets(
        context.generated_assets,
        &rendered,
        "images",
        root,
        context.workspace,
    )?;
    let candidate = parse_document(&rendered, Some(document.title()))?;
    let mut allowed = diagram_artifacts;
    allowed.extend(
        context
            .workspace
            .list_files(&root.join("images"), &[])
            .unwrap_or_default(),
    );
    let assembled = context.assembler.assemble(DocumentAssemblyRequest {
        document: &candidate,
        theme: context.theme,
        setup: context.setup,
        project: context.project,
        revision_date: context.revision_date,
        allowed_assets: &allowed,
    })?;
    let document = Path::new("inspect.html");
    context
        .workspace
        .write(&root.join(document), assembled.html.as_bytes())?;
    context
        .renderer
        .inspect_format(
            DocumentRenderRequest {
                workspace_root: root,
                document,
                output: Path::new("inspect.paginated.html"),
                setup: context.setup,
            },
            context.operation,
        )
        .await
}

/// Normalises a document title to a single line of single-spaced words.
///
/// Collapsing whitespace rather than only trimming is what the slides path already
/// does, and it matters for the same reason: the renderer strips the `# {title}`
/// heading by matching it, and a title carrying a stray space or an embedded
/// newline never matches the heading it produced. Trimming alone left internal
/// whitespace and newlines intact.
pub(super) fn validate_title(title: String) -> Result<String> {
    let normalized = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(SfumatoError::validation("Document title cannot be empty"));
    }
    if slug::slugify(&normalized).is_empty() {
        return Err(SfumatoError::validation(
            "Document title cannot produce an artifact name",
        ));
    }
    Ok(normalized)
}

/// Removes an outer Markdown code fence a model may wrap its answer in.
pub(super) fn strip_markdown_fence(value: &str) -> String {
    let trimmed = value.trim();
    for opener in ["```markdown", "```md", "```"] {
        if let Some(rest) = trimmed.strip_prefix(opener)
            && let Some(inner) = rest.trim_start_matches(['\n', '\r']).strip_suffix("```")
        {
            return inner.trim_end().to_owned();
        }
    }
    trimmed.to_owned()
}

pub(super) fn build_source_bundle(
    documents: &[crate::sources::SourceDocument],
    limit: usize,
) -> String {
    let mut output = String::new();
    for document in documents {
        let header = format!("\n\n## {}\n", document.path.display());
        if output.chars().count() + header.chars().count() >= limit {
            break;
        }
        output.push_str(&header);
        let remaining = limit.saturating_sub(output.chars().count());
        output.extend(document.content.chars().take(remaining));
    }
    if output.is_empty() {
        "No source files were supplied.".into()
    } else {
        output
    }
}

pub(super) fn summarize_tools(
    tools: &[crate::providers::ToolDefinition],
) -> Vec<GenerationToolSummary> {
    tools
        .iter()
        .map(|tool| GenerationToolSummary {
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
        })
        .collect()
}

pub(super) fn format_tokens(tokens: &BTreeMap<String, String>) -> String {
    if tokens.is_empty() {
        return "unspecified".into();
    }
    tokens
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn emit_stage(
    sink: &Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    stage: GenerationStage,
    profile: Option<&str>,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::StageStarted {
            stage,
            profile: profile.map(str::to_owned),
        });
    }
}

/// Rejects a path that would escape the transaction staging root.
pub(super) fn ensure_inside(root: &Path, path: &Path) -> Result<()> {
    if !path.starts_with(root) {
        return Err(SfumatoError::validation(format!(
            "Document artifact {} would escape its workspace",
            path.display()
        )));
    }
    Ok(())
}
