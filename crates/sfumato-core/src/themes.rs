//! Reusable visual theme records and application service.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::sfumato_bail as bail;
use crate::{
    config::ProjectConfig,
    errors::SfumatoResult as Result,
    repositories::{ProjectRepository, ThemeRepository},
};

/// Name of the theme installed during user setup.
pub const DEFAULT_THEME: &str = "sfumato-default";
/// Current theme package manifest schema.
pub const THEME_SCHEMA_VERSION: u32 = 1;

/// Portable theme package manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeManifest {
    pub schema_version: u32,
    pub name: String,
    pub description: String,
    pub tokens: ThemeTokens,
    pub adapters: ThemeAdapters,
}

/// Semantic visual tokens shared by resource renderers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeTokens {
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub fonts: BTreeMap<String, String>,
}

/// Renderer-specific files provided by a theme package.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeAdapters {
    pub marp_css: PathBuf,
    #[serde(default)]
    pub html: Option<HtmlThemeAdapter>,
}

/// Files used by the standalone HTML page renderer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HtmlThemeAdapter {
    pub shell: PathBuf,
    pub css: PathBuf,
    #[serde(default)]
    pub script: Option<PathBuf>,
}

/// A validated theme package and its infrastructure-owned root.
#[derive(Clone, Debug)]
pub struct ThemePackage {
    pub root: PathBuf,
    pub manifest: ThemeManifest,
}

/// Compact theme data used by lists and selectors.
#[derive(Clone, Debug)]
pub struct ThemeSummary {
    pub name: String,
}

impl ThemePackage {
    /// Returns the validated Marp adapter path.
    pub fn marp_css_path(&self) -> PathBuf {
        self.root.join(&self.manifest.adapters.marp_css)
    }
}

/// Coordinates theme packages and project theme selection.
pub struct ThemeService {
    repository: Arc<dyn ThemeRepository>,
    project_repository: Arc<dyn ProjectRepository>,
}

impl ThemeService {
    /// Creates the service from persistence ports.
    pub fn new(
        repository: Arc<dyn ThemeRepository>,
        project_repository: Arc<dyn ProjectRepository>,
    ) -> Self {
        Self {
            repository,
            project_repository,
        }
    }

    /// Installs the bundled default package when absent.
    pub fn install_default(&self) -> Result<ThemePackage> {
        self.repository.install_default()
    }

    /// Creates a custom package from the bundled scaffold.
    pub fn create(&self, name: &str) -> Result<ThemePackage> {
        validate_theme_name(name)?;
        self.repository.create(name)
    }

    /// Lists installed packages.
    pub fn list(&self) -> Result<Vec<ThemeSummary>> {
        self.repository.list()
    }

    /// Lists installed package names.
    pub fn names(&self) -> Result<Vec<String>> {
        Ok(self.list()?.into_iter().map(|theme| theme.name).collect())
    }

    /// Assigns a validated theme to the active or selected project.
    pub fn use_for_project(
        &self,
        name: &str,
        requested_project: Option<&str>,
    ) -> Result<ProjectConfig> {
        self.repository.load(name)?;
        let snapshot = self.project_repository.load_snapshot(requested_project)?;
        let mut project = snapshot.value;
        project.theme = name.to_string();
        self.project_repository
            .save_if_revision(&project, &snapshot.revision)?;
        Ok(project)
    }

    /// Resolves one installed and validated package.
    pub fn resolve(&self, name: &str) -> Result<ThemePackage> {
        validate_theme_name(name)?;
        self.repository.load(name)
    }

    /// Imports a DESIGN.md document into a new reusable theme package.
    pub fn import_design(&self, path: PathBuf, name: Option<&str>) -> Result<ThemePackage> {
        self.repository.import_design(path, name)
    }

    /// Exports one installed theme to a DESIGN.md document.
    pub fn export_design(&self, name: &str, path: PathBuf) -> Result<PathBuf> {
        validate_theme_name(name)?;
        self.repository.export_design(name, path)
    }
}

/// Validates the stable theme identifier grammar.
pub fn validate_theme_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || name.starts_with('-')
        || name.ends_with('-')
    {
        bail!("Invalid theme name '{name}'. Use lowercase letters, numbers, and hyphens.");
    }
    Ok(())
}
