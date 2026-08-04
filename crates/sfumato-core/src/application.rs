//! Curated application facade shared by presentation frontends.
//!
//! The facade owns long-lived outbound ports and resolves project-scoped
//! dependencies for each operation. CLI, TUI, MCP, and future GUI frontends
//! should build requests and consume results without constructing adapters.

use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use crate::{
    artifacts::ArtifactStore,
    config::{ConfigOverrides, EffectiveConfig, GenerationToolKind, GlobalConfig},
    config_editor::{ConfigEditor, ConfigTarget},
    connectors::{
        ConnectorAuthStatus, ConnectorDetails, ConnectorPreset, ConnectorService, ConnectorSummary,
    },
    errors::{ErrorCode, OperationStage, SfumatoError, SfumatoResult},
    filesystem::WorkspaceFileSystem,
    generation::{DocumentPageSize, GenerationRequest},
    models::{ModelDefaultChanged, ModelService, ModelSummary},
    operation::OperationContext,
    page_plugins::{
        PagePluginCatalog, PagePluginDefaultChanged, PagePluginInstallResult, PagePluginPackage,
        PagePluginService, PagePluginSource, PagePluginStatus,
    },
    project_assets::{
        ALL_THEMES, AddProjectAssetRequest, ProjectAsset, ProjectAssetCatalog,
        ProjectAssetMetadata, UpdateProjectAssetRequest,
    },
    projects::{ProjectRemoved, ProjectService, ProjectSummary},
    prompts::{
        PromptCatalog, PromptError, PromptId, PromptManager, PromptOverrideScope, PromptProvenance,
        PromptTemplateSource, PromptTemplateSummary,
    },
    providers::{
        ConnectorCapabilities, ConnectorIntrospection, ConnectorModelSummary, ConnectorStatus,
        ProviderFactory, TextGenerationEvent,
    },
    python::PythonRuntime,
    renderers::{
        DiagramRenderer, DocumentAssembler, DocumentRenderer, PageAssembler, PageInspector,
        RendererManager, RendererStatus, SlideRenderer, VideoRenderer,
    },
    repositories::{GlobalConfigRepository, ProjectRepository, ThemeRepository},
    resources::documents::{GenerateDocumentOptions, GenerateDocumentResult, generate_document},
    resources::pages::{GeneratePageOptions, GeneratePageResult, generate_page},
    resources::slides::{
        EditSlidesOptions, EditSlidesRequest, EditSlidesResult, GenerateSlidesOptions,
        GenerateSlidesResult, edit_slides, generate_slides,
    },
    resources::videos::{
        ApproveVideoReviewOptions, GenerateVideoOptions, GenerateVideoRequest, GenerateVideoResult,
        approve_video_review, generate_video,
    },
    secrets::{SecretStore, SecretValue},
    setup::{SetupService, UserSetupRequest, UserSetupResult},
    sources::SourceReader,
    templates::{
        GenerationTemplate, GenerationTemplateCatalog, GenerationTemplateSummary, TemplateKind,
    },
    themes::{ThemePackage, ThemeService, ThemeSummary},
    tools::GenerationToolFactory,
};

/// Resolves one immutable effective configuration snapshot for an operation.
pub trait EffectiveConfigResolver: Send + Sync {
    /// Loads and validates configuration using the supplied command overrides.
    fn resolve(&self, overrides: ConfigOverrides) -> SfumatoResult<EffectiveConfig>;
}

/// Creates a layered prompt catalog scoped to one project root.
pub trait PromptCatalogFactory: Send + Sync {
    /// Builds the catalog used for one operation.
    fn for_project(&self, project_root: &Path) -> SfumatoResult<Arc<dyn PromptCatalog>>;
}

/// Complete request for the slide-generation use case.
pub struct GenerateSlidesCommand {
    /// Job lifecycle, cancellation, deadline, and event context.
    pub operation: OperationContext,
    /// Configuration and command-line precedence overrides.
    pub config: ConfigOverrides,
    /// Resource instruction, sources, and model selections.
    pub request: GenerationRequest,
    /// Optional explicit deck title.
    pub title: Option<String>,
    /// Optional reusable structural template package.
    pub template: Option<String>,
    /// Resolve and render prompts without invoking providers or writing files.
    pub dry_run: bool,
    /// Run semantic and layout review.
    pub review: bool,
    /// Legacy detailed generation events consumed by current frontends.
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

/// Complete request for the paginated document-generation use case.
pub struct GenerateDocumentCommand {
    /// Job lifecycle, cancellation, deadline, and event context.
    pub operation: OperationContext,
    /// Configuration and command-line precedence overrides.
    pub config: ConfigOverrides,
    /// Resource instruction, sources, and model selections.
    pub request: GenerationRequest,
    /// Optional explicit document title.
    pub title: Option<String>,
    /// Optional reusable structural template package.
    pub template: Option<String>,
    /// Sheet override; the theme decides when absent.
    pub page_size: Option<DocumentPageSize>,
    /// Table-of-contents override; the theme decides when absent.
    pub table_of_contents: Option<bool>,
    /// Cover-page override; the theme decides when absent.
    pub cover: Option<bool>,
    /// Resolve and render prompts without invoking providers or writing files.
    pub dry_run: bool,
    /// Run semantic review and focused page-format repair.
    pub review: bool,
    /// Legacy detailed generation events consumed by current frontends.
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

/// Complete request for the standalone page-generation use case.
pub struct GeneratePageCommand {
    /// Cancellation, deadline, and event context.
    pub operation: OperationContext,
    /// Project, theme, model, and output overrides.
    pub config: ConfigOverrides,
    /// Instruction and optional grounding sources.
    pub request: GenerationRequest,
    /// Optional explicit page title.
    pub title: Option<String>,
    /// Optional reusable structural template package.
    pub template: Option<String>,
    /// Selected installed page plugin identifiers.
    pub plugins: Vec<String>,
    /// Project utility plugins disabled for this request.
    pub disabled_plugins: Vec<String>,
    /// Optional exclusive UI component-library override; an empty string disables project UI.
    pub ui: Option<String>,
    /// Resolves the request without invoking models or writing artifacts.
    pub dry_run: bool,
    /// Enables semantic and browser-focused model repair.
    pub review: bool,
    /// Optional frontend observer for provider progress events.
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

/// Complete request for standalone video generation.
pub struct GenerateVideoCommand {
    /// Cancellation, deadline, and event context.
    pub operation: OperationContext,
    /// Project, theme, model, output, and tool overrides.
    pub config: ConfigOverrides,
    /// Instruction and optional grounding sources.
    pub request: GenerationRequest,
    /// Engine-specific generation parameters.
    pub video: GenerateVideoRequest,
    /// Resolve prompts and dependencies without model calls or writes.
    pub dry_run: bool,
    /// Enable semantic plan review and focused source repair.
    pub review: bool,
    /// Optional frontend observer for detailed progress.
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

/// Renders a previously paused and manually approved Hyperframe review session.
pub struct ApproveVideoReviewCommand {
    /// Cancellation and deadline context for the final render.
    pub operation: OperationContext,
    /// Project and output overrides.
    pub config: ConfigOverrides,
    /// Immutable session selected for approval.
    pub review_id: String,
}

/// Locates a persisted Hyperframe review session for a preview frontend.
pub struct PreviewVideoReviewCommand {
    /// Project used to resolve managed review storage.
    pub config: ConfigOverrides,
    /// Immutable session selected for preview.
    pub review_id: String,
}

/// Complete request for the focused slide-editing use case.
pub struct EditSlidesCommand {
    /// Job lifecycle, cancellation, deadline, and event context.
    pub operation: OperationContext,
    /// Configuration and command-line precedence overrides.
    pub config: ConfigOverrides,
    /// Existing artifact and focused editing instruction.
    pub request: EditSlidesRequest,
    /// Legacy detailed generation events consumed by current frontends.
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

/// Outbound ports and paths required by [`SfumatoApplication`].
///
/// Keeping the dependency bundle explicit makes production and test
/// composition readable as the application facade grows.
pub struct SfumatoApplicationDependencies {
    /// Effective configuration resolver.
    pub config: Arc<dyn EffectiveConfigResolver>,
    /// Project-scoped prompt catalog factory.
    pub prompts: Arc<dyn PromptCatalogFactory>,
    /// Prompt template management port.
    pub prompt_manager: Arc<dyn PromptManager>,
    /// Transactional artifact store.
    pub artifacts: Arc<dyn ArtifactStore>,
    /// Model provider factory.
    pub providers: Arc<dyn ProviderFactory>,
    /// Connector-native model discovery.
    pub connector_introspection: Arc<dyn ConnectorIntrospection>,
    /// Diagram renderer.
    pub diagrams: Arc<dyn DiagramRenderer>,
    /// Slide renderer.
    pub slides: Arc<dyn SlideRenderer>,
    /// Standalone page compiler and validator.
    pub page_assembler: Arc<dyn PageAssembler>,
    /// Browser-backed page inspector.
    pub page_inspector: Arc<dyn PageInspector>,
    /// Markdown-to-printable-HTML document compiler.
    pub document_assembler: Arc<dyn DocumentAssembler>,
    /// Browser-backed document paginator and PDF exporter.
    pub document_renderer: Arc<dyn DocumentRenderer>,
    /// Hyperframe and Manim renderer adapter.
    pub video_renderer: Arc<dyn VideoRenderer>,
    /// Explicit optional-renderer lifecycle.
    pub renderer_manager: Arc<dyn RendererManager>,
    /// Managed Python environments for generated code.
    pub python_runtime: Arc<dyn PythonRuntime>,
    /// Offline page plugin catalog.
    pub page_plugins: Arc<dyn PagePluginCatalog>,
    /// Supported-plugin metadata and public-CDN materializer.
    pub page_plugin_source: Arc<dyn PagePluginSource>,
    /// Reusable generation-template catalog.
    pub templates: Arc<dyn GenerationTemplateCatalog>,
    /// Portable reusable project-asset catalog.
    pub project_assets: Arc<dyn ProjectAssetCatalog>,
    /// Source material reader.
    pub sources: Arc<dyn SourceReader>,
    /// Generation tool factory.
    pub tools: Arc<dyn GenerationToolFactory>,
    /// Theme repository.
    pub themes: Arc<dyn ThemeRepository>,
    /// Project repository.
    pub projects: Arc<dyn ProjectRepository>,
    /// Global configuration repository.
    pub global_config: Arc<dyn GlobalConfigRepository>,
    /// User configuration file used by configuration commands.
    pub user_config_path: PathBuf,
    /// Workspace filesystem operations.
    pub workspace: Arc<dyn WorkspaceFileSystem>,
    /// Structured configuration editor.
    pub config_editor: Arc<dyn ConfigEditor>,
    /// Secure credential storage for connector authentication workflows.
    pub secrets: Arc<dyn SecretStore>,
}

/// Shared entry point for Sfumato application workflows.
///
/// This facade is intentionally independent from terminal presentation and
/// concrete adapter types. Production composition happens in
/// `sfumato-adapters`; tests may inject in-memory ports.
pub struct SfumatoApplication {
    config: Arc<dyn EffectiveConfigResolver>,
    prompts: Arc<dyn PromptCatalogFactory>,
    prompt_manager: Arc<dyn PromptManager>,
    artifacts: Arc<dyn ArtifactStore>,
    providers: Arc<dyn ProviderFactory>,
    connector_introspection: Arc<dyn ConnectorIntrospection>,
    diagrams: Arc<dyn DiagramRenderer>,
    slides: Arc<dyn SlideRenderer>,
    page_assembler: Arc<dyn PageAssembler>,
    page_inspector: Arc<dyn PageInspector>,
    document_assembler: Arc<dyn DocumentAssembler>,
    document_renderer: Arc<dyn DocumentRenderer>,
    video_renderer: Arc<dyn VideoRenderer>,
    renderer_manager: Arc<dyn RendererManager>,
    python_runtime: Arc<dyn PythonRuntime>,
    page_plugins: Arc<dyn PagePluginCatalog>,
    page_plugin_source: Arc<dyn PagePluginSource>,
    templates: Arc<dyn GenerationTemplateCatalog>,
    project_assets: Arc<dyn ProjectAssetCatalog>,
    sources: Arc<dyn SourceReader>,
    tools: Arc<dyn GenerationToolFactory>,
    themes: Arc<dyn ThemeRepository>,
    projects: Arc<dyn ProjectRepository>,
    global_config: Arc<dyn GlobalConfigRepository>,
    user_config_path: std::path::PathBuf,
    workspace: Arc<dyn WorkspaceFileSystem>,
    config_editor: Arc<dyn ConfigEditor>,
    secrets: Arc<dyn SecretStore>,
}

/// Presentation-neutral request for registering a reusable project artifact.
#[derive(Clone, Debug)]
pub struct AddProjectAssetCommand {
    /// Existing image file to register.
    pub source: PathBuf,
    /// Optional logical artifact name inferred from the source when absent.
    pub name: Option<String>,
    /// Human-readable purpose.
    pub description: Option<String>,
    /// Accessible description used by generated resources.
    pub alt_text: Option<String>,
    /// Semantic labels.
    pub tags: Vec<String>,
    /// Recipe used to recreate missing theme variants.
    pub generation_prompt: Option<String>,
    /// Exact theme association; defaults to the selected project theme.
    pub theme: Option<String>,
    /// Whether this concrete file works with every visual theme.
    pub all_themes: bool,
    /// Optional project override.
    pub project: Option<String>,
}

/// Presentation-neutral request for changing reusable artifact metadata.
#[derive(Clone, Debug, Default)]
pub struct UpdateProjectAssetCommand {
    /// Logical artifact name.
    pub name: String,
    /// Replacement purpose.
    pub description: Option<String>,
    /// Replacement accessible description.
    pub alt_text: Option<String>,
    /// Replacement semantic labels.
    pub tags: Option<Vec<String>>,
    /// Replacement recipe, where `Some(None)` clears the recipe.
    pub generation_prompt: Option<Option<String>>,
    /// Existing and replacement theme keys for one variant.
    pub variant_theme: Option<(String, String)>,
    /// Optional project override.
    pub project: Option<String>,
}

/// Effective state of one optional model-facing generation tool.
#[derive(Clone, Debug, serde::Serialize)]
pub struct GenerationToolStatus {
    /// Stable tool identifier.
    pub tool: GenerationToolKind,
    /// Whether the selected project enables the tool.
    pub enabled: bool,
    /// Whether a model profile exists for the required capability.
    pub model_configured: bool,
}

impl SfumatoApplication {
    /// Creates an application from its outbound ports.
    pub fn new(dependencies: SfumatoApplicationDependencies) -> Self {
        let SfumatoApplicationDependencies {
            config,
            prompts,
            prompt_manager,
            artifacts,
            providers,
            connector_introspection,
            diagrams,
            slides,
            page_assembler,
            page_inspector,
            document_assembler,
            document_renderer,
            video_renderer,
            renderer_manager,
            python_runtime,
            page_plugins,
            page_plugin_source,
            templates,
            project_assets,
            sources,
            tools,
            themes,
            projects,
            global_config,
            user_config_path,
            workspace,
            config_editor,
            secrets,
        } = dependencies;

        Self {
            config,
            prompts,
            prompt_manager,
            artifacts,
            providers,
            connector_introspection,
            diagrams,
            slides,
            page_assembler,
            page_inspector,
            document_assembler,
            document_renderer,
            video_renderer,
            renderer_manager,
            python_runtime,
            page_plugins,
            page_plugin_source,
            templates,
            project_assets,
            sources,
            tools,
            themes,
            projects,
            global_config,
            user_config_path,
            workspace,
            config_editor,
            secrets,
        }
    }

    /// Generates and transactionally commits a Marp slide resource.
    pub async fn generate_slides(
        &self,
        command: GenerateSlidesCommand,
    ) -> SfumatoResult<GenerateSlidesResult> {
        command.operation.checkpoint(OperationStage::Resolve)?;
        let config = self
            .config
            .resolve(command.config)
            .map_err(|error| error.at_stage(OperationStage::Resolve))?;
        let prompt_catalog = self
            .prompts
            .for_project(&config.project_root)
            .map_err(|error| error.at_stage(OperationStage::RenderPrompt))?;
        generate_slides(
            config,
            command.request,
            GenerateSlidesOptions {
                title: command.title,
                template: command
                    .template
                    .map(|name| self.templates.load(&name, TemplateKind::Slides))
                    .transpose()?,
                project_asset_catalog: Arc::clone(&self.project_assets),
                operation: command.operation,
                dry_run: command.dry_run,
                review: command.review,
                event_sink: command.event_sink,
                prompt_catalog,
                artifact_store: Arc::clone(&self.artifacts),
                provider_factory: Arc::clone(&self.providers),
                diagram_renderer: Arc::clone(&self.diagrams),
                slide_renderer: Arc::clone(&self.slides),
                source_reader: Arc::clone(&self.sources),
                tool_factory: Arc::clone(&self.tools),
                python_runtime: Arc::clone(&self.python_runtime),
                theme_repository: Arc::clone(&self.themes),
                workspace: Arc::clone(&self.workspace),
            },
        )
        .await
    }

    /// Generates and transactionally commits a paginated PDF document.
    pub async fn generate_document(
        &self,
        command: GenerateDocumentCommand,
    ) -> SfumatoResult<GenerateDocumentResult> {
        command.operation.checkpoint(OperationStage::Resolve)?;
        let config = self
            .config
            .resolve(command.config)
            .map_err(|error| error.at_stage(OperationStage::Resolve))?;
        let prompt_catalog = self
            .prompts
            .for_project(&config.project_root)
            .map_err(|error| error.at_stage(OperationStage::RenderPrompt))?;
        generate_document(
            config,
            command.request,
            GenerateDocumentOptions {
                operation: command.operation,
                title: command.title,
                template: command
                    .template
                    .map(|name| self.templates.load(&name, TemplateKind::Document))
                    .transpose()?,
                dry_run: command.dry_run,
                review: command.review,
                page_size: command.page_size,
                table_of_contents: command.table_of_contents,
                cover: command.cover,
                event_sink: command.event_sink,
                prompt_catalog,
                artifact_store: Arc::clone(&self.artifacts),
                provider_factory: Arc::clone(&self.providers),
                diagram_renderer: Arc::clone(&self.diagrams),
                document_assembler: Arc::clone(&self.document_assembler),
                document_renderer: Arc::clone(&self.document_renderer),
                source_reader: Arc::clone(&self.sources),
                tool_factory: Arc::clone(&self.tools),
                python_runtime: Arc::clone(&self.python_runtime),
                theme_repository: Arc::clone(&self.themes),
                workspace: Arc::clone(&self.workspace),
                project_asset_catalog: Arc::clone(&self.project_assets),
            },
        )
        .await
    }

    /// Generates and transactionally commits a standalone HTML page.
    pub async fn generate_page(
        &self,
        command: GeneratePageCommand,
    ) -> SfumatoResult<GeneratePageResult> {
        command.operation.checkpoint(OperationStage::Resolve)?;
        let config = self
            .config
            .resolve(command.config)
            .map_err(|error| error.at_stage(OperationStage::Resolve))?;
        let prompt_catalog = self
            .prompts
            .for_project(&config.project_root)
            .map_err(|error| error.at_stage(OperationStage::RenderPrompt))?;
        let mut plugins = config.page.plugins.clone();
        plugins.retain(|id| !command.disabled_plugins.contains(id));
        plugins.extend(command.plugins);
        plugins.sort();
        plugins.dedup();
        let ui = command.ui.or(config.page.ui.clone());
        if let Some(ui) = ui.filter(|value| !value.is_empty()) {
            plugins.push(ui);
        }
        generate_page(
            config,
            command.request,
            GeneratePageOptions {
                operation: command.operation,
                title: command.title,
                template: command
                    .template
                    .map(|name| self.templates.load(&name, TemplateKind::Page))
                    .transpose()?,
                project_asset_catalog: Arc::clone(&self.project_assets),
                plugins,
                dry_run: command.dry_run,
                review: command.review,
                event_sink: command.event_sink,
                prompt_catalog,
                artifact_store: Arc::clone(&self.artifacts),
                provider_factory: Arc::clone(&self.providers),
                source_reader: Arc::clone(&self.sources),
                tool_factory: Arc::clone(&self.tools),
                python_runtime: Arc::clone(&self.python_runtime),
                theme_repository: Arc::clone(&self.themes),
                plugin_catalog: Arc::clone(&self.page_plugins),
                page_assembler: Arc::clone(&self.page_assembler),
                page_inspector: Arc::clone(&self.page_inspector),
                workspace: Arc::clone(&self.workspace),
            },
        )
        .await
    }

    /// Generates and transactionally commits a standalone MP4 resource.
    pub async fn generate_video(
        &self,
        command: GenerateVideoCommand,
    ) -> SfumatoResult<GenerateVideoResult> {
        command.operation.checkpoint(OperationStage::Resolve)?;
        let config = self
            .config
            .resolve(command.config)
            .map_err(|error| error.at_stage(OperationStage::Resolve))?;
        let prompt_catalog = self
            .prompts
            .for_project(&config.project_root)
            .map_err(|error| error.at_stage(OperationStage::RenderPrompt))?;
        generate_video(
            config,
            command.request,
            command.video,
            GenerateVideoOptions {
                operation: command.operation,
                dry_run: command.dry_run,
                review: command.review,
                event_sink: command.event_sink,
                prompt_catalog,
                artifact_store: Arc::clone(&self.artifacts),
                provider_factory: Arc::clone(&self.providers),
                source_reader: Arc::clone(&self.sources),
                tool_factory: Arc::clone(&self.tools),
                python_runtime: Arc::clone(&self.python_runtime),
                theme_repository: Arc::clone(&self.themes),
                video_renderer: Arc::clone(&self.video_renderer),
                workspace: Arc::clone(&self.workspace),
                project_asset_catalog: Arc::clone(&self.project_assets),
            },
        )
        .await
    }

    /// Renders the exact source bundle persisted by `--visual-review`.
    pub async fn approve_video_review(
        &self,
        command: ApproveVideoReviewCommand,
    ) -> SfumatoResult<GenerateVideoResult> {
        command.operation.checkpoint(OperationStage::Resolve)?;
        let publish_root_override = command.config.publish_dir.is_some();
        let config = self
            .config
            .resolve(command.config)
            .map_err(|error| error.at_stage(OperationStage::Resolve))?;
        approve_video_review(
            config,
            &command.review_id,
            ApproveVideoReviewOptions {
                operation: command.operation,
                artifact_store: Arc::clone(&self.artifacts),
                video_renderer: Arc::clone(&self.video_renderer),
                workspace: Arc::clone(&self.workspace),
                publish_root_override,
            },
        )
        .await
    }

    /// Returns the immutable session source directory used by Hyperframes preview.
    pub fn preview_video_review(
        &self,
        command: PreviewVideoReviewCommand,
    ) -> SfumatoResult<PathBuf> {
        if command.review_id.is_empty()
            || command.review_id.contains(['/', '\\'])
            || command.review_id.contains("..")
        {
            return Err(SfumatoError::validation("Invalid video review identifier"));
        }
        let config = self.config.resolve(command.config)?;
        let root = self
            .artifacts
            .project_root(&config.project_name)?
            .join("review-sessions")
            .join(command.review_id)
            .join("source");
        if !self.workspace.is_file(&root.join("index.html")) {
            return Err(SfumatoError::validation(
                "Video review session is missing its Hyperframe source",
            ));
        }
        Ok(root)
    }

    /// Lists project-scoped optional tools independently from page plugins.
    pub fn list_generation_tools(
        &self,
        project: Option<String>,
    ) -> SfumatoResult<Vec<GenerationToolStatus>> {
        let effective = self.config.resolve(ConfigOverrides {
            project,
            ..ConfigOverrides::default()
        })?;
        // Iterated from the enum so a new tool reaches `sfumato tool list`, the
        // TUI, and automation at once.
        Ok(GenerationToolKind::ALL
            .into_iter()
            .map(|tool| GenerationToolStatus {
                tool,
                enabled: effective.generation_tool_enabled(tool),
                model_configured: tool
                    .capability()
                    .is_none_or(|capability| effective.resolve_model(capability).is_ok()),
            })
            .collect())
    }

    /// Persists one project-scoped generation-tool default.
    pub fn set_generation_tool(
        &self,
        tool: GenerationToolKind,
        enabled: bool,
        project: Option<&str>,
    ) -> SfumatoResult<crate::config::ProjectConfig> {
        let mut snapshot = self.projects.load_snapshot(project)?;
        snapshot.value.generation_tools.0.insert(tool, enabled);
        self.projects
            .save_if_revision(&snapshot.value, &snapshot.revision)?;
        Ok(snapshot.value)
    }

    /// Lists the install and dependency status of optional local video renderers.
    pub async fn list_renderers(
        &self,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<RendererStatus>> {
        self.renderer_manager.list(operation).await
    }

    /// Installs one optional local video renderer explicitly.
    pub async fn install_renderer(
        &self,
        id: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<RendererStatus> {
        self.renderer_manager.install(id, operation).await
    }

    /// Removes one optional local video renderer managed by Sfumato.
    pub fn remove_renderer(&self, id: &str) -> SfumatoResult<RendererStatus> {
        self.renderer_manager.remove(id)
    }

    /// Checks one optional local video renderer and all of its dependencies.
    pub async fn doctor_renderers(
        &self,
        id: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<RendererStatus>> {
        self.renderer_manager.doctor(id, operation).await
    }

    /// Lists installed offline page plugins.
    pub fn list_installed_page_plugins(
        &self,
    ) -> SfumatoResult<Vec<crate::page_plugins::PagePluginSummary>> {
        self.page_plugins.list()
    }

    /// Resolves one installed offline page plugin.
    pub fn show_page_plugin(&self, id: &str) -> SfumatoResult<PagePluginPackage> {
        self.page_plugins.load(id)
    }

    fn page_plugin_service(&self) -> PagePluginService {
        PagePluginService::new(
            Arc::clone(&self.page_plugin_source),
            Arc::clone(&self.page_plugins),
            Arc::clone(&self.projects),
        )
    }

    /// Lists every supported plugin with installation and project state.
    pub async fn list_page_plugins(
        &self,
        project: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<PagePluginStatus>> {
        self.page_plugin_service().list(project, operation).await
    }

    /// Shows one supported plugin with installation and project state.
    pub async fn show_page_plugin_status(
        &self,
        id: &str,
        project: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<PagePluginStatus> {
        self.list_page_plugins(project, operation)
            .await?
            .into_iter()
            .find(|status| status.plugin.id == id)
            .ok_or_else(|| SfumatoError::not_found(format!("Unknown page plugin '{id}'")))
    }

    /// Installs a supported plugin release and its dependencies.
    pub async fn install_page_plugin(
        &self,
        id: &str,
        version: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<PagePluginInstallResult> {
        self.page_plugin_service()
            .install(id, version, operation)
            .await
    }

    /// Updates an installed plugin to its latest supported release.
    pub async fn update_page_plugin(
        &self,
        id: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<PagePluginInstallResult> {
        self.page_plugin_service().update(id, operation).await
    }

    /// Enables an installed plugin for a project.
    pub fn enable_page_plugin(
        &self,
        id: &str,
        project: Option<&str>,
    ) -> SfumatoResult<PagePluginDefaultChanged> {
        self.page_plugin_service().enable(id, project)
    }

    /// Disables a plugin for a project.
    pub fn disable_page_plugin(
        &self,
        id: &str,
        project: Option<&str>,
    ) -> SfumatoResult<PagePluginDefaultChanged> {
        self.page_plugin_service().disable(id, project)
    }

    /// Lists installed reusable structural templates.
    pub fn list_templates(
        &self,
        kind: Option<TemplateKind>,
    ) -> SfumatoResult<Vec<GenerationTemplateSummary>> {
        self.templates.list(kind)
    }

    /// Loads one reusable structural template.
    pub fn show_template(
        &self,
        name: &str,
        kind: TemplateKind,
    ) -> SfumatoResult<GenerationTemplate> {
        self.templates.load(name, kind)
    }

    /// Creates one reusable structural template package.
    pub fn create_template(
        &self,
        name: &str,
        kind: TemplateKind,
        source: Option<PathBuf>,
    ) -> SfumatoResult<GenerationTemplate> {
        self.templates.create(name, kind, source)
    }

    /// Imports a Google DESIGN.md file as a Sfumato theme.
    pub fn import_theme_design(
        &self,
        path: PathBuf,
        name: Option<&str>,
    ) -> SfumatoResult<ThemePackage> {
        ThemeService::new(Arc::clone(&self.themes), Arc::clone(&self.projects))
            .import_design(path, name)
    }

    /// Exports a Sfumato theme as a Google DESIGN.md file.
    pub fn export_theme_design(&self, name: &str, path: PathBuf) -> SfumatoResult<PathBuf> {
        ThemeService::new(Arc::clone(&self.themes), Arc::clone(&self.projects))
            .export_design(name, path)
    }

    /// Lists reusable assets for the active or explicitly selected project.
    pub fn list_project_assets(&self, project: Option<&str>) -> SfumatoResult<Vec<ProjectAsset>> {
        let root = self.selected_project_root(project)?;
        self.project_assets.list(&root)
    }

    /// Loads one reusable project asset.
    pub fn show_project_asset(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> SfumatoResult<ProjectAsset> {
        let root = self.selected_project_root(project)?;
        self.project_assets.load(&root, name)
    }

    /// Copies and registers one reusable project asset.
    pub fn add_project_asset(
        &self,
        command: AddProjectAssetCommand,
    ) -> SfumatoResult<ProjectAsset> {
        if command.all_themes && command.theme.is_some() {
            return Err(SfumatoError::validation(
                "Use either an exact artifact theme or all themes, not both",
            ));
        }
        let registry = self.projects.registry()?;
        let (project_name, root) = registry.selected(command.project.as_deref())?;
        let project_config = self.projects.load(Some(&project_name))?;
        let theme = if command.all_themes {
            ALL_THEMES.to_string()
        } else {
            command.theme.unwrap_or(project_config.theme)
        };
        let description = command
            .description
            .unwrap_or_else(|| "Reusable project visual artifact".into());
        self.project_assets.add(
            &root,
            AddProjectAssetRequest {
                source: &command.source,
                name: command.name.as_deref(),
                theme: &theme,
                metadata: ProjectAssetMetadata {
                    alt_text: command.alt_text.unwrap_or_else(|| description.clone()),
                    description,
                    tags: command.tags,
                    generation_prompt: command.generation_prompt,
                },
            },
        )
    }

    /// Updates semantic metadata or a variant theme selector.
    pub fn update_project_asset(
        &self,
        command: UpdateProjectAssetCommand,
    ) -> SfumatoResult<ProjectAsset> {
        let root = self.selected_project_root(command.project.as_deref())?;
        self.project_assets.update(
            &root,
            &command.name,
            UpdateProjectAssetRequest {
                description: command.description,
                alt_text: command.alt_text,
                tags: command.tags,
                generation_prompt: command.generation_prompt,
                variant_theme: command.variant_theme,
            },
        )
    }

    /// Removes one reusable project asset and its managed copy.
    pub fn remove_project_asset(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> SfumatoResult<ProjectAsset> {
        let root = self.selected_project_root(project)?;
        self.project_assets.remove(&root, name)
    }

    fn selected_project_root(&self, project: Option<&str>) -> SfumatoResult<PathBuf> {
        let registry = self.projects.registry()?;
        registry.selected(project).map(|(_, root)| root)
    }

    /// Applies focused content patches and commits a new deck revision.
    pub async fn edit_slides(&self, command: EditSlidesCommand) -> SfumatoResult<EditSlidesResult> {
        command.operation.checkpoint(OperationStage::Resolve)?;
        let config = self
            .config
            .resolve(command.config)
            .map_err(|error| error.at_stage(OperationStage::Resolve))?;
        let prompt_catalog = self
            .prompts
            .for_project(&config.project_root)
            .map_err(|error| error.at_stage(OperationStage::RenderPrompt))?;
        edit_slides(
            config,
            command.request,
            EditSlidesOptions {
                operation: command.operation,
                event_sink: command.event_sink,
                prompt_catalog,
                artifact_store: Arc::clone(&self.artifacts),
                provider_factory: Arc::clone(&self.providers),
                diagram_renderer: Arc::clone(&self.diagrams),
                slide_renderer: Arc::clone(&self.slides),
                source_reader: Arc::clone(&self.sources),
                theme_repository: Arc::clone(&self.themes),
                workspace: Arc::clone(&self.workspace),
            },
        )
        .await
        .map_err(|error| {
            if error.stage.is_some() {
                error
            } else {
                error.at_stage(OperationStage::Edit)
            }
        })
    }

    /// Creates a reusable theme package from the bundled scaffold.
    pub fn create_theme(&self, name: &str) -> SfumatoResult<ThemePackage> {
        public_result(self.theme_service().create(name), ErrorCode::Validation)
    }

    /// Lists installed reusable themes.
    pub fn list_themes(&self) -> SfumatoResult<Vec<ThemeSummary>> {
        public_result(self.theme_service().list(), ErrorCode::Config)
    }

    /// Resolves an installed reusable theme.
    pub fn show_theme(&self, name: &str) -> SfumatoResult<ThemePackage> {
        public_result(self.theme_service().resolve(name), ErrorCode::NotFound)
    }

    /// Selects a theme for the active or explicitly named project.
    pub fn use_theme(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> SfumatoResult<crate::config::ProjectConfig> {
        public_result(
            self.theme_service().use_for_project(name, project),
            ErrorCode::Validation,
        )
    }

    fn theme_service(&self) -> ThemeService {
        ThemeService::new(Arc::clone(&self.themes), Arc::clone(&self.projects))
    }

    /// Returns whether the user-global configuration already exists.
    pub fn user_config_exists(&self) -> bool {
        self.setup_service().user_config_exists()
    }

    /// Returns the user-global configuration path.
    pub fn user_config_path(&self) -> &Path {
        &self.user_config_path
    }

    /// Validates and persists initial user configuration and the default theme.
    pub fn setup_user(&self, config: GlobalConfig) -> SfumatoResult<UserSetupResult> {
        public_result(
            self.setup_service().setup_user(UserSetupRequest { config }),
            ErrorCode::Config,
        )
    }

    fn setup_service(&self) -> SetupService {
        SetupService::new(
            self.user_config_path.clone(),
            Arc::clone(&self.global_config),
            Arc::clone(&self.themes),
        )
    }

    /// Registers a project and optionally makes it active.
    pub fn init_project(
        &self,
        name: String,
        path: PathBuf,
        activate: bool,
    ) -> SfumatoResult<crate::config::ProjectConfig> {
        public_result(
            self.project_service().init(name, path, activate),
            ErrorCode::Validation,
        )
    }

    /// Lists registered projects.
    pub fn list_projects(&self) -> SfumatoResult<Vec<ProjectSummary>> {
        public_result(self.project_service().list(), ErrorCode::Config)
    }

    /// Loads the active or explicitly selected project.
    pub fn show_project(
        &self,
        project: Option<&str>,
    ) -> SfumatoResult<crate::config::ProjectConfig> {
        public_result(self.project_service().show(project), ErrorCode::NotFound)
    }

    /// Changes the globally active project.
    pub fn use_project(&self, name: &str) -> SfumatoResult<String> {
        public_result(
            self.project_service().use_project(name),
            ErrorCode::NotFound,
        )
    }

    /// Removes a project from the registry without deleting its files.
    pub fn remove_project(&self, name: &str) -> SfumatoResult<ProjectRemoved> {
        public_result(self.project_service().remove(name), ErrorCode::NotFound)
    }

    /// Lists configured model profiles.
    pub fn list_models(&self) -> SfumatoResult<Vec<ModelSummary>> {
        public_result(
            self.model_service().map(|service| service.list()),
            ErrorCode::Config,
        )
    }

    /// Loads one model profile.
    pub fn show_model(&self, name: &str) -> SfumatoResult<crate::config::ModelProfile> {
        public_result(
            self.model_service()
                .and_then(|service| service.profile(name)),
            ErrorCode::NotFound,
        )
    }

    /// Adds a model profile.
    pub fn add_model(
        &self,
        name: String,
        connector: String,
        model_id: String,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> SfumatoResult<crate::config::ModelProfile> {
        public_result(
            self.model_service().and_then(|mut service| {
                service.add(name, connector, model_id, capabilities, options)
            }),
            ErrorCode::Validation,
        )
    }

    /// Edits supplied fields on one model profile.
    pub fn edit_model(
        &self,
        name: &str,
        connector: Option<String>,
        model_id: Option<String>,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> SfumatoResult<crate::config::ModelProfile> {
        public_result(
            self.model_service().and_then(|mut service| {
                service.edit(name, connector, model_id, capabilities, options)
            }),
            ErrorCode::Validation,
        )
    }

    /// Removes an unreferenced model profile.
    pub fn remove_model(&self, name: &str) -> SfumatoResult<String> {
        public_result(
            self.model_service()
                .and_then(|mut service| service.remove(name)),
            ErrorCode::Validation,
        )
    }

    /// Selects a model profile for a capability or role.
    pub fn use_model(
        &self,
        selector: &str,
        profile: &str,
        project: Option<&str>,
    ) -> SfumatoResult<ModelDefaultChanged> {
        public_result(
            self.model_service()
                .and_then(|mut service| service.use_default(selector, profile, project)),
            ErrorCode::Validation,
        )
    }

    /// Lists configured connectors.
    pub fn list_connectors(&self) -> SfumatoResult<Vec<ConnectorSummary>> {
        public_result(
            self.connector_service().map(|service| service.list()),
            ErrorCode::Config,
        )
    }

    /// Loads one connector connection.
    pub fn show_connector(&self, name: &str) -> SfumatoResult<ConnectorDetails> {
        public_result(
            self.connector_service()
                .and_then(|service| service.show(name)),
            ErrorCode::NotFound,
        )
    }

    /// Discovers models exposed by one connector using its native protocol.
    pub async fn list_connector_models(
        &self,
        name: &str,
        operation: OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        operation.checkpoint(OperationStage::Resolve)?;
        let snapshot = self.global_config.load_snapshot()?;
        let connector = snapshot.value.connectors.get(name).ok_or_else(|| {
            SfumatoError::not_found(format_args!("Connector '{name}' was not found"))
        })?;
        self.connector_introspection
            .list_models(name, connector, &operation)
            .await
    }

    /// Reports optional provider-native features for one configured connector.
    pub fn connector_capabilities(&self, name: &str) -> SfumatoResult<ConnectorCapabilities> {
        let snapshot = self.global_config.load_snapshot()?;
        let connector = snapshot.value.connectors.get(name).ok_or_else(|| {
            SfumatoError::not_found(format_args!("Connector '{name}' was not found"))
        })?;
        Ok(self.connector_introspection.capabilities(connector))
    }

    /// Reads provider-native account, usage, or local-runtime status.
    pub async fn connector_status(
        &self,
        name: &str,
        operation: OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        operation.checkpoint(OperationStage::Resolve)?;
        let snapshot = self.global_config.load_snapshot()?;
        let connector = snapshot.value.connectors.get(name).ok_or_else(|| {
            SfumatoError::not_found(format_args!("Connector '{name}' was not found"))
        })?;
        self.connector_introspection
            .status(name, connector, &operation)
            .await
    }

    /// Creates or replaces a connector preset.
    pub fn setup_connector(
        &self,
        preset: ConnectorPreset,
        name: Option<String>,
        api_key_env: Option<String>,
    ) -> SfumatoResult<ConnectorSummary> {
        public_result(
            self.connector_service()
                .and_then(|mut service| service.setup(preset, name, api_key_env)),
            ErrorCode::Validation,
        )
    }

    /// Saves a connector credential in secure storage and selects it in configuration.
    pub async fn login_connector(
        &self,
        name: &str,
        secret: SecretValue,
    ) -> SfumatoResult<ConnectorAuthStatus> {
        public_result(
            async {
                let mut service = self.connector_service()?;
                service.login(name, secret).await
            }
            .await,
            ErrorCode::Validation,
        )
    }

    /// Reports whether one connector's configured credential is currently available.
    pub async fn connector_auth_status(&self, name: &str) -> SfumatoResult<ConnectorAuthStatus> {
        public_result(
            async {
                let service = self.connector_service()?;
                service.auth_status(name).await
            }
            .await,
            ErrorCode::Config,
        )
    }

    /// Removes a stored connector credential and clears its configuration reference.
    pub async fn logout_connector(&self, name: &str) -> SfumatoResult<ConnectorAuthStatus> {
        public_result(
            async {
                let mut service = self.connector_service()?;
                service.logout(name).await
            }
            .await,
            ErrorCode::Config,
        )
    }

    fn project_service(&self) -> ProjectService {
        ProjectService::new(Arc::clone(&self.projects))
    }

    fn model_service(&self) -> SfumatoResult<ModelService> {
        ModelService::new(Arc::clone(&self.global_config), Arc::clone(&self.projects))
    }

    fn connector_service(&self) -> SfumatoResult<ConnectorService> {
        ConnectorService::new(Arc::clone(&self.global_config), Arc::clone(&self.secrets))
    }

    /// Shows a complete configuration scope or one dotted key.
    pub fn show_config(
        &self,
        target: ConfigTarget,
        project: Option<String>,
        key: Option<String>,
    ) -> SfumatoResult<String> {
        public_result(
            self.config_editor.show(target, project, key),
            ErrorCode::Config,
        )
    }

    /// Sets one validated dotted configuration key.
    pub fn set_config(
        &self,
        target: ConfigTarget,
        project: Option<String>,
        key: &str,
        value: &str,
    ) -> SfumatoResult<PathBuf> {
        public_result(
            self.config_editor.set(target, project, key, value),
            ErrorCode::Config,
        )
    }

    /// Deletes one validated dotted configuration key.
    pub fn delete_config(
        &self,
        target: ConfigTarget,
        project: Option<String>,
        key: &str,
    ) -> SfumatoResult<PathBuf> {
        public_result(
            self.config_editor.delete(target, project, key),
            ErrorCode::Config,
        )
    }

    /// Resolves effective configuration for non-generation presentation needs.
    pub fn resolve_config(&self, overrides: ConfigOverrides) -> SfumatoResult<EffectiveConfig> {
        public_result(self.config.resolve(overrides), ErrorCode::Config)
    }

    /// Lists prompt templates resolved for the active or selected project.
    pub fn list_prompts(
        &self,
        project: Option<String>,
    ) -> SfumatoResult<Vec<PromptTemplateSummary>> {
        let root = self.prompt_project_root(project)?;
        self.prompt_manager.list(&root).map_err(public_prompt_error)
    }

    /// Loads one unrendered prompt template for presentation.
    /// Parses a prompt identifier into the typed error model.
    ///
    /// Parsing used to happen in the presentation layer, where a `PromptError`
    /// propagated as an untyped error and reached the user with no code at all,
    /// unlike every other "does not exist" failure.
    pub fn parse_prompt_id(&self, id: &str) -> SfumatoResult<PromptId> {
        PromptId::from_str(id).map_err(public_prompt_error)
    }

    /// Reads one resolved prompt template and its provenance.
    pub fn show_prompt(
        &self,
        id: &str,
        project: Option<String>,
    ) -> SfumatoResult<PromptTemplateSource> {
        let id = self.parse_prompt_id(id)?;
        let root = self.prompt_project_root(project)?;
        self.prompt_manager
            .source(&root, id)
            .map_err(public_prompt_error)
    }

    /// Creates one user or project prompt override.
    pub fn customize_prompt(
        &self,
        id: &str,
        scope: PromptOverrideScope,
        project: Option<String>,
    ) -> SfumatoResult<PathBuf> {
        let id = self.parse_prompt_id(id)?;
        let root = self.prompt_project_root(project)?;
        self.prompt_manager
            .customize(&root, id, scope)
            .map_err(public_prompt_error)
    }

    /// Validates every prompt resolved for the active or selected project.
    pub fn validate_prompts(
        &self,
        project: Option<String>,
    ) -> SfumatoResult<Vec<PromptProvenance>> {
        let root = self.prompt_project_root(project)?;
        self.prompt_manager
            .validate(&root)
            .map_err(public_prompt_error)
    }

    fn prompt_project_root(&self, project: Option<String>) -> SfumatoResult<PathBuf> {
        public_result(
            self.config
                .resolve(ConfigOverrides {
                    project,
                    ..Default::default()
                })
                .map(|config| config.project_root),
            ErrorCode::Config,
        )
    }
}

fn public_result<T>(result: SfumatoResult<T>, _fallback_code: ErrorCode) -> SfumatoResult<T> {
    result
}

fn public_prompt_error(error: PromptError) -> SfumatoError {
    error.into()
}

#[cfg(test)]
// Test bodies live outside the implementation while retaining access to the
// private application-facade helpers.
#[path = "../tests/unit/application.rs"]
mod tests;
