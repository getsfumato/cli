//! Curated application facade shared by presentation frontends.
//!
//! The facade owns long-lived outbound ports and resolves project-scoped
//! dependencies for each operation. CLI, TUI, MCP, and future GUI frontends
//! should build requests and consume results without constructing adapters.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    artifacts::ArtifactStore,
    config::{ConfigOverrides, EffectiveConfig, GlobalConfig},
    config_editor::{ConfigEditor, ConfigTarget},
    connectors::{ConnectorDetails, ConnectorPreset, ConnectorService, ConnectorSummary},
    errors::{ErrorCode, OperationStage, SfumatoError, SfumatoResult},
    filesystem::WorkspaceFileSystem,
    generation::GenerationRequest,
    models::{ModelDefaultChanged, ModelService, ModelSummary},
    operation::OperationContext,
    page_plugins::{PagePluginCatalog, PagePluginPackage, PagePluginSummary},
    projects::{ProjectRemoved, ProjectService, ProjectSummary},
    prompts::{
        PromptCatalog, PromptError, PromptId, PromptManager, PromptOverrideScope, PromptProvenance,
        PromptTemplateSource, PromptTemplateSummary,
    },
    providers::{ProviderFactory, TextGenerationEvent},
    renderers::{DiagramRenderer, PageAssembler, PageInspector, SlideRenderer},
    repositories::{GlobalConfigRepository, ProjectRepository, ThemeRepository},
    resources::pages::{GeneratePageOptions, GeneratePageResult, generate_page},
    resources::slides::{
        EditSlidesOptions, EditSlidesRequest, EditSlidesResult, GenerateSlidesOptions,
        GenerateSlidesResult, edit_slides, generate_slides,
    },
    setup::{SetupService, UserSetupRequest, UserSetupResult},
    sources::SourceReader,
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
    /// Resolve and render prompts without invoking providers or writing files.
    pub dry_run: bool,
    /// Run semantic and layout review.
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
    /// Selected bundled page plugin identifiers.
    pub plugins: Vec<String>,
    /// Resolves the request without invoking models or writing artifacts.
    pub dry_run: bool,
    /// Enables semantic and browser-focused model repair.
    pub review: bool,
    /// Optional frontend observer for provider progress events.
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
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
    /// Diagram renderer.
    pub diagrams: Arc<dyn DiagramRenderer>,
    /// Slide renderer.
    pub slides: Arc<dyn SlideRenderer>,
    /// Standalone page compiler and validator.
    pub page_assembler: Arc<dyn PageAssembler>,
    /// Browser-backed page inspector.
    pub page_inspector: Arc<dyn PageInspector>,
    /// Offline page plugin catalog.
    pub page_plugins: Arc<dyn PagePluginCatalog>,
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
    diagrams: Arc<dyn DiagramRenderer>,
    slides: Arc<dyn SlideRenderer>,
    page_assembler: Arc<dyn PageAssembler>,
    page_inspector: Arc<dyn PageInspector>,
    page_plugins: Arc<dyn PagePluginCatalog>,
    sources: Arc<dyn SourceReader>,
    tools: Arc<dyn GenerationToolFactory>,
    themes: Arc<dyn ThemeRepository>,
    projects: Arc<dyn ProjectRepository>,
    global_config: Arc<dyn GlobalConfigRepository>,
    user_config_path: std::path::PathBuf,
    workspace: Arc<dyn WorkspaceFileSystem>,
    config_editor: Arc<dyn ConfigEditor>,
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
            diagrams,
            slides,
            page_assembler,
            page_inspector,
            page_plugins,
            sources,
            tools,
            themes,
            projects,
            global_config,
            user_config_path,
            workspace,
            config_editor,
        } = dependencies;

        Self {
            config,
            prompts,
            prompt_manager,
            artifacts,
            providers,
            diagrams,
            slides,
            page_assembler,
            page_inspector,
            page_plugins,
            sources,
            tools,
            themes,
            projects,
            global_config,
            user_config_path,
            workspace,
            config_editor,
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
                theme_repository: Arc::clone(&self.themes),
                workspace: Arc::clone(&self.workspace),
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
        generate_page(
            config,
            command.request,
            GeneratePageOptions {
                operation: command.operation,
                title: command.title,
                plugins: command.plugins,
                dry_run: command.dry_run,
                review: command.review,
                event_sink: command.event_sink,
                prompt_catalog,
                artifact_store: Arc::clone(&self.artifacts),
                provider_factory: Arc::clone(&self.providers),
                source_reader: Arc::clone(&self.sources),
                tool_factory: Arc::clone(&self.tools),
                theme_repository: Arc::clone(&self.themes),
                plugin_catalog: Arc::clone(&self.page_plugins),
                page_assembler: Arc::clone(&self.page_assembler),
                page_inspector: Arc::clone(&self.page_inspector),
                workspace: Arc::clone(&self.workspace),
            },
        )
        .await
    }

    /// Lists bundled offline page plugins.
    pub fn list_page_plugins(&self) -> SfumatoResult<Vec<PagePluginSummary>> {
        self.page_plugins.list()
    }

    /// Resolves one bundled offline page plugin.
    pub fn show_page_plugin(&self, id: &str) -> SfumatoResult<PagePluginPackage> {
        self.page_plugins.load(id)
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

    /// Creates or replaces a connector preset.
    pub fn setup_connector(
        &self,
        preset: ConnectorPreset,
        name: Option<String>,
        api_key_env: String,
    ) -> SfumatoResult<ConnectorSummary> {
        public_result(
            self.connector_service()
                .and_then(|mut service| service.setup(preset, name, api_key_env)),
            ErrorCode::Validation,
        )
    }

    fn project_service(&self) -> ProjectService {
        ProjectService::new(Arc::clone(&self.projects))
    }

    fn model_service(&self) -> SfumatoResult<ModelService> {
        ModelService::new(Arc::clone(&self.global_config), Arc::clone(&self.projects))
    }

    fn connector_service(&self) -> SfumatoResult<ConnectorService> {
        ConnectorService::new(Arc::clone(&self.global_config))
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
    pub fn show_prompt(
        &self,
        id: PromptId,
        project: Option<String>,
    ) -> SfumatoResult<PromptTemplateSource> {
        let root = self.prompt_project_root(project)?;
        self.prompt_manager
            .source(&root, id)
            .map_err(public_prompt_error)
    }

    /// Creates one user or project prompt override.
    pub fn customize_prompt(
        &self,
        id: PromptId,
        scope: PromptOverrideScope,
        project: Option<String>,
    ) -> SfumatoResult<PathBuf> {
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
