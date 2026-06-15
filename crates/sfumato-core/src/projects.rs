use std::path::PathBuf;

use anyhow::Result;

use crate::{
    config::ProjectConfig,
    repositories::{FilesystemProjectRepository, ProjectRepository},
};

#[derive(Clone, Debug)]
pub struct ProjectSummary {
    pub name: String,
    pub path: PathBuf,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct ProjectRemoved {
    pub name: String,
}

pub struct ProjectService {
    repository: Box<dyn ProjectRepository>,
}

impl ProjectService {
    pub fn load() -> Result<Self> {
        Ok(Self::new(Box::new(
            FilesystemProjectRepository::default_path()?,
        )))
    }

    pub fn new(repository: Box<dyn ProjectRepository>) -> Self {
        Self { repository }
    }

    pub fn init(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig> {
        self.repository.register(name, path, activate)
    }

    pub fn list(&self) -> Result<Vec<ProjectSummary>> {
        Ok(self
            .repository
            .list()?
            .into_iter()
            .map(|(name, project, active)| ProjectSummary {
                name,
                path: project.path,
                active,
            })
            .collect())
    }

    pub fn show(&self, requested: Option<&str>) -> Result<String> {
        let project = self.repository.load(requested)?;
        toml::to_string_pretty(&project).map_err(Into::into)
    }

    pub fn use_project(&self, name: &str) -> Result<String> {
        self.repository.set_active(name)
    }

    pub fn remove(&self, name: &str) -> Result<ProjectRemoved> {
        let project = self.repository.remove(name)?;
        Ok(ProjectRemoved { name: project.name })
    }
}
