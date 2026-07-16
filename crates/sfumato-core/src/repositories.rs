//! Persistence ports consumed by application services.

use std::path::PathBuf;

use anyhow::Result;

use crate::{
    config::{GlobalConfig, ProjectConfig, ProjectRegistry, RegisteredProject},
    themes::{ThemePackage, ThemeSummary},
};

/// Persisted value paired with an opaque storage revision token.
#[derive(Clone, Debug)]
pub struct RepositorySnapshot<T> {
    /// Persisted value decoded into its application type.
    pub value: T,
    /// Opaque revision token used for optimistic concurrency.
    pub revision: String,
}

/// User-global configuration persistence.
pub trait GlobalConfigRepository: Send + Sync {
    /// Returns whether an explicit global configuration exists.
    fn exists(&self) -> bool;
    /// Loads the global configuration or its repository-defined default.
    fn load(&self) -> Result<GlobalConfig>;
    /// Validates and persists the complete global configuration.
    fn save(&self, config: &GlobalConfig) -> Result<()>;
    /// Loads the value together with its current storage revision.
    fn load_snapshot(&self) -> Result<RepositorySnapshot<GlobalConfig>> {
        Ok(RepositorySnapshot {
            value: self.load()?,
            revision: "unversioned".to_string(),
        })
    }
    /// Saves only when `expected` still matches the current revision.
    fn save_if_revision(&self, config: &GlobalConfig, _expected: &str) -> Result<String> {
        self.save(config)?;
        Ok("unversioned".to_string())
    }
}

/// Project registry and portable project configuration persistence.
pub trait ProjectRepository: Send + Sync {
    /// Loads the registry containing active and named projects.
    fn registry(&self) -> Result<ProjectRegistry>;
    /// Lists registered projects and whether each one is active.
    fn list(&self) -> Result<Vec<(String, RegisteredProject, bool)>>;
    /// Loads the named project, or the active project when omitted.
    fn load(&self, name: Option<&str>) -> Result<ProjectConfig>;
    /// Persists one portable project configuration.
    fn save(&self, project: &ProjectConfig) -> Result<()>;
    /// Loads a project together with its current storage revision.
    fn load_snapshot(&self, name: Option<&str>) -> Result<RepositorySnapshot<ProjectConfig>> {
        Ok(RepositorySnapshot {
            value: self.load(name)?,
            revision: "unversioned".to_string(),
        })
    }
    /// Saves a project only when `expected` matches the current revision.
    fn save_if_revision(&self, project: &ProjectConfig, _expected: &str) -> Result<String> {
        self.save(project)?;
        Ok("unversioned".to_string())
    }
    /// Registers a project root and optionally activates it.
    fn register(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig>;
    /// Selects the named registered project as active.
    fn set_active(&self, name: &str) -> Result<String>;
    /// Removes a registry entry without deleting project files.
    fn remove(&self, name: &str) -> Result<ProjectConfig>;
}

/// User-global reusable theme package persistence.
pub trait ThemeRepository: Send + Sync {
    /// Lists installed reusable theme packages.
    fn list(&self) -> Result<Vec<ThemeSummary>>;
    /// Loads and validates the named theme package.
    fn load(&self, name: &str) -> Result<ThemePackage>;
    /// Creates a theme package from the bundled scaffold.
    fn create(&self, name: &str) -> Result<ThemePackage>;
    /// Installs or loads the bundled default theme.
    fn install_default(&self) -> Result<ThemePackage>;
}
