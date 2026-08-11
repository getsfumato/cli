//! Registering, listing and selecting projects.

use std::{path::PathBuf, sync::Arc};

use crate::{
    config::ProjectConfig, errors::SfumatoResult as Result, repositories::ProjectRepository,
};

/// One row of the project registry, as a caller displays it.
#[derive(Clone, Debug)]
pub struct ProjectSummary {
    /// Registry name, which is how every command refers to the project.
    pub name: String,
    /// Canonical root the project was registered at.
    pub path: PathBuf,
    /// Whether this is the project used when a command omits `--project`.
    pub active: bool,
    /// Whether the registered path still holds a readable project config.
    ///
    /// A moved or deleted directory leaves the entry behind, and listing it as
    /// if it were fine meant the state only surfaced when a later command
    /// failed on it.
    pub available: bool,
}

/// Confirmation that a registration is gone.
///
/// Only the registration: the project directory, its `.sfumato` folder, its sources
/// and its generated revisions are all left untouched.
#[derive(Clone, Debug)]
pub struct ProjectRemoved {
    /// The name that was removed, as it was stored.
    pub name: String,
}

/// Project registry use cases.
pub struct ProjectService {
    repository: Arc<dyn ProjectRepository>,
}

impl ProjectService {
    /// Creates the service over the registry port.
    pub fn new(repository: Arc<dyn ProjectRepository>) -> Self {
        Self { repository }
    }

    /// Registers a project at `path`, writing its `.sfumato/project.toml`.
    ///
    /// `activate` makes it the project used when `--project` is omitted.
    pub fn init(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig> {
        self.repository.register(name, path, activate)
    }

    /// Every registered project, each marked with whether it is still readable.
    pub fn list(&self) -> Result<Vec<ProjectSummary>> {
        Ok(self
            .repository
            .list()?
            .into_iter()
            .map(|(name, project, active)| ProjectSummary {
                available: self.repository.load(Some(&name)).is_ok(),
                name,
                path: project.path,
                active,
            })
            .collect())
    }

    /// One project's own configuration; `None` reads the active project.
    pub fn show(&self, requested: Option<&str>) -> Result<ProjectConfig> {
        self.repository.load(requested)
    }

    /// Makes a registered project the active one, returning its name.
    ///
    /// Affects only which project commands default to; it does not edit the
    /// project's own configuration.
    pub fn use_project(&self, name: &str) -> Result<String> {
        self.repository.set_active(name)
    }

    /// Removes a registration, leaving everything on disk in place.
    pub fn remove(&self, name: &str) -> Result<ProjectRemoved> {
        let name = self.repository.remove(name)?;
        Ok(ProjectRemoved { name })
    }
}
