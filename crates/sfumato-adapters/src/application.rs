//! Production composition for the Sfumato application facade.

use std::{path::Path, sync::Arc};

use anyhow::Result;
use sfumato_core::{
    application::{
        EffectiveConfigResolver, PromptCatalogFactory, SfumatoApplication,
        SfumatoApplicationDependencies,
    },
    config::{ConfigOverrides, EffectiveConfig},
    errors::{SfumatoError, SfumatoResult},
    prompts::PromptCatalog,
    repositories::{GlobalConfigRepository, ProjectRepository},
};

use crate::{
    artifacts::FilesystemArtifactStore,
    config_editor::TomlConfigEditor,
    config_files::ConfigPaths,
    filesystem::LocalWorkspaceFileSystem,
    page_plugins::{CdnPagePluginSource, FilesystemPagePluginCatalog},
    pages::{ChromiumPageInspector, StandalonePageAssembler},
    project_assets::FilesystemProjectAssetCatalog,
    prompts::{LayeredPromptCatalog, LayeredPromptManager},
    providers::AdapterProviderFactory,
    renderers::{MarpCliRenderer, MermaidCliRenderer},
    repositories::{FilesystemGlobalConfigRepository, FilesystemProjectRepository},
    secrets::SystemSecretStore,
    sources::FilesystemSourceReader,
    templates::FilesystemGenerationTemplateCatalog,
    themes::FilesystemThemeRepository,
    tools::FilesystemGenerationToolFactory,
};

/// Filesystem-backed effective configuration resolver.
#[derive(Clone)]
pub struct FilesystemConfigResolver {
    global: Arc<dyn GlobalConfigRepository>,
    projects: Arc<dyn ProjectRepository>,
}

impl FilesystemConfigResolver {
    /// Creates a resolver from global and project persistence ports.
    pub fn new(
        global: Arc<dyn GlobalConfigRepository>,
        projects: Arc<dyn ProjectRepository>,
    ) -> Self {
        Self { global, projects }
    }
}

impl EffectiveConfigResolver for FilesystemConfigResolver {
    fn resolve(&self, overrides: ConfigOverrides) -> SfumatoResult<EffectiveConfig> {
        let global = self.global.load()?;
        let registry = self.projects.registry()?;
        let (selected_name, project_root) = registry.selected(overrides.project.as_deref())?;
        let project = self.projects.load(Some(&selected_name))?;
        EffectiveConfig::from_parts(global, selected_name, project_root, project, overrides)
    }
}

/// Layered project/user/bundled prompt catalog factory.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayeredPromptCatalogFactory;

impl PromptCatalogFactory for LayeredPromptCatalogFactory {
    fn for_project(&self, project_root: &Path) -> SfumatoResult<Arc<dyn PromptCatalog>> {
        LayeredPromptCatalog::for_project(project_root)
            .map(|catalog| Arc::new(catalog) as Arc<dyn PromptCatalog>)
            .map_err(|error| SfumatoError::config(format_args!("{error:#}")))
    }
}

/// Builds the production application used by CLI and TUI presentation.
pub fn production_application() -> Result<SfumatoApplication> {
    let paths = ConfigPaths::discover()?;
    let user_config_path = paths.user_config;
    let themes = Arc::new(FilesystemThemeRepository::default_path()?);
    let projects = Arc::new(FilesystemProjectRepository::default_path()?);
    let global_config = Arc::new(FilesystemGlobalConfigRepository::new(
        user_config_path.clone(),
    ));
    let config_resolver: Arc<dyn EffectiveConfigResolver> = Arc::new(
        FilesystemConfigResolver::new(global_config.clone(), projects.clone()),
    );
    let config_editor = Arc::new(TomlConfigEditor::new(
        user_config_path.clone(),
        global_config.clone(),
        projects.clone(),
        config_resolver.clone(),
    ));
    let secrets = Arc::new(SystemSecretStore::default());
    let secret_resolver: Arc<dyn sfumato_core::secrets::SecretResolver> = secrets.clone();
    let secret_store: Arc<dyn sfumato_core::secrets::SecretStore> = secrets;
    let providers = Arc::new(AdapterProviderFactory::new(secret_resolver));
    Ok(SfumatoApplication::new(SfumatoApplicationDependencies {
        config: config_resolver,
        prompts: Arc::new(LayeredPromptCatalogFactory),
        prompt_manager: Arc::new(LayeredPromptManager),
        artifacts: Arc::new(FilesystemArtifactStore::default_path()?),
        providers: providers.clone(),
        connector_introspection: providers,
        diagrams: Arc::new(MermaidCliRenderer),
        slides: Arc::new(MarpCliRenderer),
        page_assembler: Arc::new(StandalonePageAssembler),
        page_inspector: Arc::new(ChromiumPageInspector),
        page_plugins: Arc::new(FilesystemPagePluginCatalog::default_path()?),
        page_plugin_source: Arc::new(CdnPagePluginSource::new()?),
        templates: Arc::new(FilesystemGenerationTemplateCatalog::default_path()?),
        project_assets: Arc::new(FilesystemProjectAssetCatalog),
        sources: Arc::new(FilesystemSourceReader),
        tools: Arc::new(FilesystemGenerationToolFactory),
        themes,
        projects,
        global_config,
        user_config_path,
        workspace: Arc::new(LocalWorkspaceFileSystem),
        config_editor,
        secrets: secret_store,
    }))
}
