use std::{path::PathBuf, sync::Arc};

use crate::{
    config::ProjectConfig, errors::SfumatoResult as Result, repositories::ProjectRepository,
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
    repository: Arc<dyn ProjectRepository>,
}

impl ProjectService {
    pub fn new(repository: Arc<dyn ProjectRepository>) -> Self {
        Self { repository }
    }

    pub fn init(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig> {
        Ok(self.repository.register(name, path, activate)?)
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

    pub fn show(&self, requested: Option<&str>) -> Result<ProjectConfig> {
        Ok(self.repository.load(requested)?)
    }

    pub fn use_project(&self, name: &str) -> Result<String> {
        Ok(self.repository.set_active(name)?)
    }

    pub fn remove(&self, name: &str) -> Result<ProjectRemoved> {
        let project = self.repository.remove(name)?;
        Ok(ProjectRemoved { name: project.name })
    }
}
