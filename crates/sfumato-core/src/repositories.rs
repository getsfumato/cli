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
    pub value: T,
    pub revision: String,
}

/// User-global configuration persistence.
pub trait GlobalConfigRepository: Send + Sync {
    fn exists(&self) -> bool;
    fn load(&self) -> Result<GlobalConfig>;
    fn save(&self, config: &GlobalConfig) -> Result<()>;
    fn load_snapshot(&self) -> Result<RepositorySnapshot<GlobalConfig>> {
        Ok(RepositorySnapshot {
            value: self.load()?,
            revision: "unversioned".to_string(),
        })
    }
    fn save_if_revision(&self, config: &GlobalConfig, _expected: &str) -> Result<String> {
        self.save(config)?;
        Ok("unversioned".to_string())
    }
}

/// Project registry and portable project configuration persistence.
pub trait ProjectRepository: Send + Sync {
    fn registry(&self) -> Result<ProjectRegistry>;
    fn list(&self) -> Result<Vec<(String, RegisteredProject, bool)>>;
    fn load(&self, name: Option<&str>) -> Result<ProjectConfig>;
    fn save(&self, project: &ProjectConfig) -> Result<()>;
    fn load_snapshot(&self, name: Option<&str>) -> Result<RepositorySnapshot<ProjectConfig>> {
        Ok(RepositorySnapshot {
            value: self.load(name)?,
            revision: "unversioned".to_string(),
        })
    }
    fn save_if_revision(&self, project: &ProjectConfig, _expected: &str) -> Result<String> {
        self.save(project)?;
        Ok("unversioned".to_string())
    }
    fn register(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig>;
    fn set_active(&self, name: &str) -> Result<String>;
    fn remove(&self, name: &str) -> Result<ProjectConfig>;
}

/// User-global reusable theme package persistence.
pub trait ThemeRepository: Send + Sync {
    fn list(&self) -> Result<Vec<ThemeSummary>>;
    fn load(&self, name: &str) -> Result<ThemePackage>;
    fn create(&self, name: &str) -> Result<ThemePackage>;
    fn install_default(&self) -> Result<ThemePackage>;
}
