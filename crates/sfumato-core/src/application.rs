//! Curated application facade shared by presentation frontends.
//!
//! The facade owns long-lived outbound ports and resolves project-scoped
//! dependencies for each operation. CLI, TUI, MCP, and future GUI frontends
//! should build requests and consume results without constructing adapters.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;

use crate::{
    artifacts::ArtifactStore,
    config::{ConfigOverrides, EffectiveConfig, GlobalConfig},
    config_editor::{ConfigEditor, ConfigTarget},
    connectors::{ConnectorPreset, ConnectorService, ConnectorSummary},
    filesystem::WorkspaceFileSystem,
    generation::GenerationRequest,
    models::{ModelDefaultChanged, ModelService, ModelSummary},
    projects::{ProjectRemoved, ProjectService, ProjectSummary},
    prompts::PromptCatalog,
    providers::{ProviderFactory, TextGenerationEvent},
    renderers::{DiagramRenderer, SlideRenderer},
    repositories::{GlobalConfigRepository, ProjectRepository, ThemeRepository},
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
    fn resolve(&self, overrides: ConfigOverrides) -> Result<EffectiveConfig>;
}

/// Creates a layered prompt catalog scoped to one project root.
pub trait PromptCatalogFactory: Send + Sync {
    /// Builds the catalog used for one operation.
    fn for_project(&self, project_root: &Path) -> Result<Arc<dyn PromptCatalog>>;
}

/// Complete request for the slide-generation use case.
pub struct GenerateSlidesCommand {
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

/// Complete request for the focused slide-editing use case.
pub struct EditSlidesCommand {
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
    /// Transactional artifact store.
    pub artifacts: Arc<dyn ArtifactStore>,
    /// Model provider factory.
    pub providers: Arc<dyn ProviderFactory>,
    /// Diagram renderer.
    pub diagrams: Arc<dyn DiagramRenderer>,
    /// Slide renderer.
    pub slides: Arc<dyn SlideRenderer>,
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
    artifacts: Arc<dyn ArtifactStore>,
    providers: Arc<dyn ProviderFactory>,
    diagrams: Arc<dyn DiagramRenderer>,
    slides: Arc<dyn SlideRenderer>,
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
            artifacts,
            providers,
            diagrams,
            slides,
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
            artifacts,
            providers,
            diagrams,
            slides,
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
    ) -> Result<GenerateSlidesResult> {
        let config = self.config.resolve(command.config)?;
        let prompt_catalog = self.prompts.for_project(&config.project_root)?;
        generate_slides(
            config,
            command.request,
            GenerateSlidesOptions {
                title: command.title,
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

    /// Applies focused content patches and commits a new deck revision.
    pub async fn edit_slides(&self, command: EditSlidesCommand) -> Result<EditSlidesResult> {
        let config = self.config.resolve(command.config)?;
        let prompt_catalog = self.prompts.for_project(&config.project_root)?;
        edit_slides(
            config,
            command.request,
            EditSlidesOptions {
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
    }

    /// Creates a reusable theme package from the bundled scaffold.
    pub fn create_theme(&self, name: &str) -> Result<ThemePackage> {
        self.theme_service().create(name)
    }

    /// Lists installed reusable themes.
    pub fn list_themes(&self) -> Result<Vec<ThemeSummary>> {
        self.theme_service().list()
    }

    /// Resolves an installed reusable theme.
    pub fn show_theme(&self, name: &str) -> Result<ThemePackage> {
        self.theme_service().resolve(name)
    }

    /// Selects a theme for the active or explicitly named project.
    pub fn use_theme(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> Result<crate::config::ProjectConfig> {
        self.theme_service().use_for_project(name, project)
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
    pub fn setup_user(&self, config: GlobalConfig) -> Result<UserSetupResult> {
        self.setup_service().setup_user(UserSetupRequest { config })
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
    ) -> Result<crate::config::ProjectConfig> {
        self.project_service().init(name, path, activate)
    }

    /// Lists registered projects.
    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>> {
        self.project_service().list()
    }

    /// Loads the active or explicitly selected project.
    pub fn show_project(&self, project: Option<&str>) -> Result<crate::config::ProjectConfig> {
        self.project_service().show(project)
    }

    /// Changes the globally active project.
    pub fn use_project(&self, name: &str) -> Result<String> {
        self.project_service().use_project(name)
    }

    /// Removes a project from the registry without deleting its files.
    pub fn remove_project(&self, name: &str) -> Result<ProjectRemoved> {
        self.project_service().remove(name)
    }

    /// Lists configured model profiles.
    pub fn list_models(&self) -> Result<Vec<ModelSummary>> {
        Ok(self.model_service()?.list())
    }

    /// Loads one model profile.
    pub fn show_model(&self, name: &str) -> Result<crate::config::ModelProfile> {
        self.model_service()?.profile(name)
    }

    /// Adds a model profile.
    pub fn add_model(
        &self,
        name: String,
        connector: String,
        model_id: String,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> Result<crate::config::ModelProfile> {
        self.model_service()?
            .add(name, connector, model_id, capabilities, options)
    }

    /// Edits supplied fields on one model profile.
    pub fn edit_model(
        &self,
        name: &str,
        connector: Option<String>,
        model_id: Option<String>,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> Result<crate::config::ModelProfile> {
        self.model_service()?
            .edit(name, connector, model_id, capabilities, options)
    }

    /// Removes an unreferenced model profile.
    pub fn remove_model(&self, name: &str) -> Result<String> {
        self.model_service()?.remove(name)
    }

    /// Selects a model profile for a capability or role.
    pub fn use_model(
        &self,
        selector: &str,
        profile: &str,
        project: Option<&str>,
    ) -> Result<ModelDefaultChanged> {
        self.model_service()?
            .use_default(selector, profile, project)
    }

    /// Lists configured connectors.
    pub fn list_connectors(&self) -> Result<Vec<ConnectorSummary>> {
        Ok(self.connector_service()?.list())
    }

    /// Loads one connector connection.
    pub fn show_connector(
        &self,
        name: &str,
    ) -> Result<crate::config::OpenAiCompatibleConnectorConfig> {
        self.connector_service()?.show(name)
    }

    /// Creates or replaces a connector preset.
    pub fn setup_connector(
        &self,
        preset: ConnectorPreset,
        name: Option<String>,
        api_key_env: String,
    ) -> Result<ConnectorSummary> {
        self.connector_service()?.setup(preset, name, api_key_env)
    }

    fn project_service(&self) -> ProjectService {
        ProjectService::new(Arc::clone(&self.projects))
    }

    fn model_service(&self) -> Result<ModelService> {
        ModelService::new(Arc::clone(&self.global_config), Arc::clone(&self.projects))
    }

    fn connector_service(&self) -> Result<ConnectorService> {
        ConnectorService::new(Arc::clone(&self.global_config))
    }

    /// Shows a complete configuration scope or one dotted key.
    pub fn show_config(
        &self,
        target: ConfigTarget,
        project: Option<String>,
        key: Option<String>,
    ) -> Result<String> {
        self.config_editor.show(target, project, key)
    }

    /// Sets one validated dotted configuration key.
    pub fn set_config(
        &self,
        target: ConfigTarget,
        project: Option<String>,
        key: &str,
        value: &str,
    ) -> Result<PathBuf> {
        self.config_editor.set(target, project, key, value)
    }

    /// Deletes one validated dotted configuration key.
    pub fn delete_config(
        &self,
        target: ConfigTarget,
        project: Option<String>,
        key: &str,
    ) -> Result<PathBuf> {
        self.config_editor.delete(target, project, key)
    }

    /// Resolves effective configuration for non-generation presentation needs.
    pub fn resolve_config(&self, overrides: ConfigOverrides) -> Result<EffectiveConfig> {
        self.config.resolve(overrides)
    }
}
