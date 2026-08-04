//! Installable offline JavaScript plugin contracts and workflows.

use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    catalogs::CatalogListing,
    errors::{SfumatoError, SfumatoResult},
    operation::OperationContext,
    repositories::ProjectRepository,
};

/// Current metadata-only CDN plugin-registry schema.
pub const PAGE_PLUGIN_REGISTRY_SCHEMA_VERSION: u32 = 2;
/// Current downloadable plugin-package schema.
pub const PAGE_PLUGIN_PACKAGE_SCHEMA_VERSION: u32 = 1;

/// Public metadata for one installed page plugin.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PagePluginSummary {
    /// Stable command-line identifier.
    pub id: String,
    /// Human-readable package name.
    pub name: String,
    /// Exact installed package version.
    pub version: String,
    /// Browser global exposed to generated JavaScript.
    pub api_global: String,
    /// SHA-256 digest of the installed runtime.
    pub runtime_hash: String,
    /// Relative installed license file.
    pub license: String,
    /// Broad catalog category such as utility, UI, or runtime.
    pub category: PagePluginCategory,
    /// Runtime dependencies resolved before this plugin.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Broad model-facing role of a page plugin.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PagePluginCategory {
    /// General visualization or animation helper.
    Utility,
    /// React component library or its required runtime.
    Ui,
    /// Internal runtime dependency normally selected transitively.
    Runtime,
}

/// Installed plugin metadata and content loaded from the user plugin store.
#[derive(Clone, Debug)]
pub struct PagePluginPackage {
    /// Public package metadata.
    pub summary: PagePluginSummary,
    /// Model-facing usage guide.
    pub guidance: String,
    /// Offline JavaScript runtime injected into generated pages.
    pub runtime_javascript: String,
    /// Optional stylesheet injected before page-specific styles.
    pub stylesheet: String,
}

/// One immutable public-CDN asset used to materialize an installed plugin.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PagePluginCdnAsset {
    /// Human-readable artifact name used in provenance and errors.
    pub name: String,
    /// Version-pinned HTTPS CDN URL.
    pub url: String,
    /// Expected SHA-256 digest of the response bytes.
    pub sha256: String,
}

impl PagePluginCdnAsset {
    fn validate(&self, plugin: &str) -> SfumatoResult<()> {
        if self.name.trim().is_empty()
            || !self.url.starts_with("https://")
            || self.sha256.len() != 64
            || !self.sha256.chars().all(|value| value.is_ascii_hexdigit())
        {
            return Err(SfumatoError::validation(format!(
                "Page plugin '{plugin}' contains invalid CDN asset metadata"
            )));
        }
        Ok(())
    }
}

/// One import replaced with a verified data module during ESM installation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PagePluginModuleImport {
    /// Exact module specifier found in the entry module.
    pub specifier: String,
    /// CDN module substituted for the specifier.
    pub asset: PagePluginCdnAsset,
}

/// Strategy used to turn public CDN responses into an offline browser runtime.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PagePluginInstallRecipe {
    /// A classic UMD/IIFE script that exposes a browser global.
    ClassicGlobal {
        /// Browser-ready runtime.
        runtime: PagePluginCdnAsset,
        /// Existing browser global copied to `api_global` after loading.
        source_global: String,
    },
    /// An ES module loaded from an integrity-checked inline data URL.
    EsmNamespace {
        /// Entry ES module.
        entry: PagePluginCdnAsset,
        /// Relative or bare imports replaced with verified data modules.
        #[serde(default)]
        imports: Vec<PagePluginModuleImport>,
    },
    /// Source definitions plus an offline Tailwind browser compiler.
    TailwindSource {
        /// Tailwind's browser runtime.
        runtime: PagePluginCdnAsset,
        /// Model-facing component definitions downloaded from the project registry.
        sources: Vec<PagePluginCdnAsset>,
    },
}

impl PagePluginInstallRecipe {
    fn validate(&self, plugin: &str) -> SfumatoResult<()> {
        match self {
            Self::ClassicGlobal {
                runtime,
                source_global,
            } => {
                runtime.validate(plugin)?;
                if source_global.trim().is_empty() {
                    return Err(SfumatoError::validation(format!(
                        "Page plugin '{plugin}' requires a source browser global"
                    )));
                }
            }
            Self::EsmNamespace { entry, imports } => {
                entry.validate(plugin)?;
                let mut specifiers = BTreeSet::new();
                for import in imports {
                    if import.specifier.trim().is_empty()
                        || !specifiers.insert(import.specifier.as_str())
                    {
                        return Err(SfumatoError::validation(format!(
                            "Page plugin '{plugin}' has invalid ESM import metadata"
                        )));
                    }
                    import.asset.validate(plugin)?;
                }
            }
            Self::TailwindSource { runtime, sources } => {
                runtime.validate(plugin)?;
                if sources.is_empty() {
                    return Err(SfumatoError::validation(format!(
                        "Page plugin '{plugin}' requires source definitions"
                    )));
                }
                for source in sources {
                    source.validate(plugin)?;
                }
            }
        }
        Ok(())
    }
}

/// One downloadable immutable release advertised by the registry.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PagePluginRelease {
    /// Exact release version.
    pub version: String,
    /// Browser API exposed after installation.
    pub api_global: String,
    /// Model-facing usage instructions.
    pub guidance: String,
    /// CDN materialization strategy.
    pub install: PagePluginInstallRecipe,
    /// Upstream license documents for every installed runtime/source.
    pub licenses: Vec<PagePluginCdnAsset>,
    /// Plugin identifiers that must be installed first.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// One supported plugin and all releases exposed by a registry snapshot.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SupportedPagePlugin {
    /// Stable plugin identifier.
    pub id: String,
    /// Human-readable plugin name.
    pub name: String,
    /// Short user-facing description.
    pub description: String,
    /// Broad plugin category.
    pub category: PagePluginCategory,
    /// Version selected by install and update when none is supplied.
    pub latest_version: String,
    /// Immutable releases known to this registry snapshot.
    pub releases: Vec<PagePluginRelease>,
}

impl SupportedPagePlugin {
    /// Finds the requested release, or the latest release when omitted.
    pub fn release(&self, requested: Option<&str>) -> SfumatoResult<&PagePluginRelease> {
        let version = requested.unwrap_or(&self.latest_version);
        self.releases
            .iter()
            .find(|release| release.version == version)
            .ok_or_else(|| {
                SfumatoError::not_found(format!(
                    "Plugin '{}' does not provide version '{version}'",
                    self.id
                ))
            })
    }
}

/// Complete validated built-in supported-plugin metadata snapshot.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PagePluginRegistry {
    /// Registry schema version.
    pub schema_version: u32,
    /// Supported plugins.
    pub plugins: Vec<SupportedPagePlugin>,
}

impl PagePluginRegistry {
    /// Validates registry identifiers, releases, and dependency references.
    pub fn validate(&self) -> SfumatoResult<()> {
        if self.schema_version != PAGE_PLUGIN_REGISTRY_SCHEMA_VERSION {
            return Err(SfumatoError::config(format!(
                "Unsupported page plugin registry schema {}; expected {}",
                self.schema_version, PAGE_PLUGIN_REGISTRY_SCHEMA_VERSION
            )));
        }
        let mut ids = BTreeSet::new();
        for plugin in &self.plugins {
            validate_plugin_id(&plugin.id)?;
            if !ids.insert(plugin.id.clone()) {
                return Err(SfumatoError::validation(format!(
                    "Page plugin registry contains duplicate plugin '{}'",
                    plugin.id
                )));
            }
            if plugin.name.trim().is_empty()
                || plugin.description.trim().is_empty()
                || plugin.latest_version.trim().is_empty()
                || plugin.releases.is_empty()
            {
                return Err(SfumatoError::validation(format!(
                    "Page plugin '{}' has incomplete registry metadata",
                    plugin.id
                )));
            }
            plugin.release(None)?;
            let mut versions = BTreeSet::new();
            for release in &plugin.releases {
                if release.version.trim().is_empty()
                    || release.api_global.trim().is_empty()
                    || release.guidance.trim().is_empty()
                {
                    return Err(SfumatoError::validation(format!(
                        "Page plugin '{}' release '{}' has invalid download metadata",
                        plugin.id, release.version
                    )));
                }
                release.install.validate(&plugin.id)?;
                if release.licenses.is_empty() {
                    return Err(SfumatoError::validation(format!(
                        "Page plugin '{}' requires upstream license metadata",
                        plugin.id
                    )));
                }
                for license in &release.licenses {
                    license.validate(&plugin.id)?;
                }
                if !versions.insert(release.version.clone()) {
                    return Err(SfumatoError::validation(format!(
                        "Page plugin '{}' repeats release '{}'",
                        plugin.id, release.version
                    )));
                }
                for dependency in &release.dependencies {
                    validate_plugin_id(dependency)?;
                }
            }
        }
        for plugin in &self.plugins {
            for release in &plugin.releases {
                for dependency in &release.dependencies {
                    if !ids.contains(dependency) {
                        return Err(SfumatoError::validation(format!(
                            "Page plugin '{}' references unsupported dependency '{dependency}'",
                            plugin.id
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Resolves one supported plugin by ID.
    pub fn plugin(&self, id: &str) -> SfumatoResult<&SupportedPagePlugin> {
        validate_plugin_id(id)?;
        self.plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| SfumatoError::not_found(format!("Unknown page plugin '{id}'")))
    }
}

/// Downloadable plugin package after transport integrity validation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DownloadedPagePluginPackage {
    /// Package schema version.
    pub schema_version: u32,
    /// Stable plugin ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Exact package version.
    pub version: String,
    /// Browser global exposed by the runtime.
    pub api_global: String,
    /// Broad plugin category.
    pub category: PagePluginCategory,
    /// Installed plugin dependencies.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// SHA-256 digest of `runtime_javascript`.
    pub runtime_hash: String,
    /// Offline browser runtime.
    pub runtime_javascript: String,
    /// Optional offline CSS.
    #[serde(default)]
    pub stylesheet: String,
    /// Instructions supplied to the drafting model.
    pub guidance: String,
    /// License filename stored with the installation.
    pub license: String,
    /// Complete license text.
    pub license_text: String,
}

impl DownloadedPagePluginPackage {
    /// Validates package identity and required content before persistence.
    pub fn validate(&self) -> SfumatoResult<()> {
        validate_plugin_id(&self.id)?;
        if self.schema_version != PAGE_PLUGIN_PACKAGE_SCHEMA_VERSION {
            return Err(SfumatoError::config(format!(
                "Unsupported page plugin package schema {}; expected {}",
                self.schema_version, PAGE_PLUGIN_PACKAGE_SCHEMA_VERSION
            )));
        }
        if self.name.trim().is_empty()
            || self.version.trim().is_empty()
            || self.api_global.trim().is_empty()
            || self.runtime_hash.len() != 64
            || self.runtime_javascript.trim().is_empty()
            || self.guidance.trim().is_empty()
            || self.license.trim().is_empty()
            || self.license_text.trim().is_empty()
        {
            return Err(SfumatoError::validation(format!(
                "Page plugin package '{}' has incomplete content",
                self.id
            )));
        }
        if self.license.contains('/') || self.license.contains('\\') || self.license == "." {
            return Err(SfumatoError::validation(format!(
                "Page plugin package '{}' has an unsafe license filename",
                self.id
            )));
        }
        for dependency in &self.dependencies {
            validate_plugin_id(dependency)?;
        }
        Ok(())
    }

    /// Converts downloaded content into the runtime package used by pages.
    pub fn into_runtime_package(self) -> PagePluginPackage {
        PagePluginPackage {
            summary: PagePluginSummary {
                id: self.id,
                name: self.name,
                version: self.version,
                api_global: self.api_global,
                runtime_hash: self.runtime_hash,
                license: self.license,
                category: self.category,
                dependencies: self.dependencies,
            },
            guidance: self.guidance,
            runtime_javascript: self.runtime_javascript,
            stylesheet: self.stylesheet,
        }
    }
}

/// Supported plugin decorated with installation and project state.
#[derive(Clone, Debug)]
pub struct PagePluginStatus {
    /// Supported plugin metadata.
    pub plugin: SupportedPagePlugin,
    /// Currently selected installed version.
    pub installed_version: Option<String>,
    /// Whether the selected project enables this plugin by default.
    pub enabled: bool,
}

/// Result of installing a plugin and any missing dependencies.
#[derive(Clone, Debug)]
pub struct PagePluginInstallResult {
    /// Packages installed or selected in dependency order.
    pub packages: Vec<PagePluginSummary>,
    /// Requested top-level plugin ID.
    pub requested: String,
}

/// Result of changing project plugin defaults.
#[derive(Clone, Debug)]
pub struct PagePluginDefaultChanged {
    /// Selected project name.
    pub project: String,
    /// Plugin ID.
    pub plugin: String,
    /// New enabled state.
    pub enabled: bool,
    /// Previous UI library replaced by this selection, when applicable.
    pub replaced: Option<String>,
}

/// Supported-plugin metadata and public-CDN materialization port.
#[async_trait]
pub trait PagePluginSource: Send + Sync {
    /// Loads the supported metadata snapshot.
    async fn registry(&self, operation: &OperationContext) -> SfumatoResult<PagePluginRegistry>;
    /// Downloads verified CDN assets and materializes one offline package.
    async fn download(
        &self,
        plugin: &SupportedPagePlugin,
        release: &PagePluginRelease,
        operation: &OperationContext,
    ) -> SfumatoResult<DownloadedPagePluginPackage>;
}

/// Versioned user plugin store consumed by generation workflows.
pub trait PagePluginCatalog: Send + Sync {
    /// Lists installed packages in stable identifier order.
    /// Lists installed packages, skipping and reporting any that cannot be read.
    fn list(&self) -> SfumatoResult<CatalogListing<PagePluginSummary>>;
    /// Loads the currently selected installed release.
    fn load(&self, id: &str) -> SfumatoResult<PagePluginPackage>;
    /// Atomically installs a validated package and selects its version.
    fn install(&self, package: DownloadedPagePluginPackage) -> SfumatoResult<PagePluginPackage>;

    /// Resolves, deduplicates, and deterministically orders installed plugins.
    fn resolve(&self, ids: &[String]) -> SfumatoResult<Vec<PagePluginPackage>> {
        let mut ids = ids.to_vec();
        ids.sort();
        ids.dedup();
        let mut resolved = Vec::new();
        let mut complete = BTreeSet::new();
        let mut visiting = BTreeSet::new();
        for id in ids {
            self.resolve_dependency(&id, &mut complete, &mut visiting, &mut resolved)?;
        }
        let ui = resolved
            .iter()
            .filter(|plugin| plugin.summary.category == PagePluginCategory::Ui)
            .map(|plugin| plugin.summary.id.as_str())
            .collect::<Vec<_>>();
        if ui.len() > 1 {
            return Err(SfumatoError::validation(format!(
                "Pages can use only one UI library; selected {}",
                ui.join(", ")
            )));
        }
        Ok(resolved)
    }

    /// Resolves one installed plugin and its dependency graph in load order.
    fn resolve_dependency(
        &self,
        id: &str,
        complete: &mut BTreeSet<String>,
        visiting: &mut BTreeSet<String>,
        resolved: &mut Vec<PagePluginPackage>,
    ) -> SfumatoResult<()> {
        if complete.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.to_string()) {
            return Err(SfumatoError::validation(format!(
                "Page plugin dependency cycle contains '{id}'"
            )));
        }
        let package = self.load(id).map_err(|error| {
            if error.code == crate::errors::ErrorCode::NotFound {
                SfumatoError::not_found(format!(
                    "Page plugin '{id}' is not installed. Run `sfumato plugin install {id}`."
                ))
            } else {
                error
            }
        })?;
        for dependency in &package.summary.dependencies {
            self.resolve_dependency(dependency, complete, visiting, resolved)?;
        }
        visiting.remove(id);
        complete.insert(id.to_string());
        resolved.push(package);
        Ok(())
    }
}

/// Plugin installation, update, and project enablement workflow.
pub struct PagePluginService {
    source: Arc<dyn PagePluginSource>,
    store: Arc<dyn PagePluginCatalog>,
    projects: Arc<dyn ProjectRepository>,
}

impl PagePluginService {
    /// Creates a plugin service from transport and persistence ports.
    pub fn new(
        source: Arc<dyn PagePluginSource>,
        store: Arc<dyn PagePluginCatalog>,
        projects: Arc<dyn ProjectRepository>,
    ) -> Self {
        Self {
            source,
            store,
            projects,
        }
    }

    /// Lists every supported plugin with installation and project state.
    pub async fn list(
        &self,
        project: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<CatalogListing<PagePluginStatus>> {
        let registry = self.source.registry(operation).await?;
        let installed = self.store.list()?;
        let registry_state = self.projects.registry()?;
        let enabled = if project.is_some() || registry_state.active.is_some() {
            let project = self.projects.load(project)?;
            project
                .page
                .plugins
                .into_iter()
                .chain(project.page.ui)
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        let mut statuses = registry
            .plugins
            .into_iter()
            .map(|plugin| PagePluginStatus {
                installed_version: installed
                    .entries
                    .iter()
                    .find(|value| value.id == plugin.id)
                    .map(|value| value.version.clone()),
                enabled: enabled.contains(&plugin.id),
                plugin,
            })
            .collect::<Vec<_>>();
        statuses.sort_by(|left, right| left.plugin.id.cmp(&right.plugin.id));
        // A damaged install would otherwise read as "not installed", which is the
        // opposite of what is wrong with it.
        Ok(CatalogListing {
            entries: statuses,
            unreadable: installed.unreadable,
        })
    }

    /// Installs one supported release and all of its dependencies.
    pub async fn install(
        &self,
        id: &str,
        version: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<PagePluginInstallResult> {
        let registry = self.source.registry(operation).await?;
        let mut complete = BTreeSet::new();
        let mut visiting = BTreeSet::new();
        let mut packages = Vec::new();
        self.install_dependency(
            &registry,
            id,
            version,
            operation,
            &mut complete,
            &mut visiting,
            &mut packages,
        )
        .await?;
        Ok(PagePluginInstallResult {
            packages,
            requested: id.to_string(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn install_dependency<'a>(
        &'a self,
        registry: &'a PagePluginRegistry,
        id: &'a str,
        version: Option<&'a str>,
        operation: &'a OperationContext,
        complete: &'a mut BTreeSet<String>,
        visiting: &'a mut BTreeSet<String>,
        installed: &'a mut Vec<PagePluginSummary>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SfumatoResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if complete.contains(id) {
                return Ok(());
            }
            if !visiting.insert(id.to_string()) {
                return Err(SfumatoError::validation(format!(
                    "Page plugin registry dependency cycle contains '{id}'"
                )));
            }
            let plugin = registry.plugin(id)?;
            let release = plugin.release(version)?;
            for dependency in &release.dependencies {
                self.install_dependency(
                    registry, dependency, None, operation, complete, visiting, installed,
                )
                .await?;
            }
            let package = self.source.download(plugin, release, operation).await?;
            if package.dependencies != release.dependencies {
                return Err(SfumatoError::validation(format!(
                    "Downloaded plugin '{}' dependency metadata does not match the registry",
                    plugin.id
                )));
            }
            let package = self.store.install(package)?;
            visiting.remove(id);
            complete.insert(id.to_string());
            installed.push(package.summary);
            Ok(())
        })
    }

    /// Updates an installed plugin to the registry's latest release.
    pub async fn update(
        &self,
        id: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<PagePluginInstallResult> {
        self.store.load(id)?;
        self.install(id, None, operation).await
    }

    /// Enables an installed plugin for the active or explicitly selected project.
    pub fn enable(
        &self,
        id: &str,
        project: Option<&str>,
    ) -> SfumatoResult<PagePluginDefaultChanged> {
        validate_plugin_id(id)?;
        let package = self.store.load(id)?;
        if package.summary.category == PagePluginCategory::Runtime {
            return Err(SfumatoError::validation(format!(
                "Runtime plugin '{id}' is selected transitively and cannot be enabled directly"
            )));
        }
        let snapshot = self.projects.load_snapshot(project)?;
        let mut config = snapshot.value;
        let replaced = match package.summary.category {
            PagePluginCategory::Ui => config
                .page
                .ui
                .replace(id.to_string())
                .filter(|old| old != id),
            PagePluginCategory::Utility => {
                if !config.page.plugins.iter().any(|plugin| plugin == id) {
                    config.page.plugins.push(id.to_string());
                    config.page.plugins.sort();
                }
                None
            }
            PagePluginCategory::Runtime => unreachable!("rejected above"),
        };
        self.projects
            .save_if_revision(&config, &snapshot.revision)?;
        Ok(PagePluginDefaultChanged {
            project: config.name,
            plugin: id.to_string(),
            enabled: true,
            replaced,
        })
    }

    /// Disables a plugin for the active or explicitly selected project.
    pub fn disable(
        &self,
        id: &str,
        project: Option<&str>,
    ) -> SfumatoResult<PagePluginDefaultChanged> {
        validate_plugin_id(id)?;
        let package = self.store.load(id)?;
        let snapshot = self.projects.load_snapshot(project)?;
        let mut config = snapshot.value;
        match package.summary.category {
            PagePluginCategory::Ui => {
                if config.page.ui.as_deref() == Some(id) {
                    config.page.ui = None;
                }
            }
            PagePluginCategory::Utility => config.page.plugins.retain(|plugin| plugin != id),
            PagePluginCategory::Runtime => {
                return Err(SfumatoError::validation(format!(
                    "Runtime plugin '{id}' is managed as a dependency and cannot be disabled directly"
                )));
            }
        }
        self.projects
            .save_if_revision(&config, &snapshot.revision)?;
        Ok(PagePluginDefaultChanged {
            project: config.name,
            plugin: id.to_string(),
            enabled: false,
            replaced: None,
        })
    }
}

/// Validates stable page-plugin identifiers.
pub fn validate_plugin_id(id: &str) -> SfumatoResult<()> {
    if id.is_empty()
        || !id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || id.starts_with('-')
        || id.ends_with('-')
    {
        return Err(SfumatoError::validation(format!(
            "Invalid page plugin ID '{id}'. Use lowercase letters, numbers, and hyphens."
        )));
    }
    Ok(())
}
