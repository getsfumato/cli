//! Persistence ports consumed by application services.

use std::path::PathBuf;

use anyhow::Result;

use crate::{
    config::{GlobalConfig, ProjectConfig, ProjectRegistry, RegisteredProject},
    themes::{ThemePackage, ThemeSummary},
};

/// User-global configuration persistence.
pub trait GlobalConfigRepository: Send + Sync {
    fn load(&self) -> Result<GlobalConfig>;
    fn save(&self, config: &GlobalConfig) -> Result<()>;
}

/// Project registry and portable project configuration persistence.
pub trait ProjectRepository: Send + Sync {
    fn registry(&self) -> Result<ProjectRegistry>;
    fn list(&self) -> Result<Vec<(String, RegisteredProject, bool)>>;
    fn load(&self, name: Option<&str>) -> Result<ProjectConfig>;
    fn save(&self, project: &ProjectConfig) -> Result<()>;
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
