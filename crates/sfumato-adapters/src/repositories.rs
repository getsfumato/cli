//! Filesystem-backed configuration repositories.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sfumato_core::{
    config::{
        CONFIG_SCHEMA_VERSION, GlobalConfig, ProjectConfig, ProjectRegistry, RegisteredProject,
        validate_project_name,
    },
    repositories::{GlobalConfigRepository, ProjectRepository},
    themes::DEFAULT_THEME,
};

use crate::config_files::{ConfigPaths, project_config_path, read_versioned, write_toml};

/// Filesystem-backed global configuration repository.
#[derive(Clone, Debug)]
pub struct FilesystemGlobalConfigRepository {
    path: PathBuf,
}

impl FilesystemGlobalConfigRepository {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Result<Self> {
        Ok(Self::new(ConfigPaths::discover()?.user_config))
    }
}

impl GlobalConfigRepository for FilesystemGlobalConfigRepository {
    fn load(&self) -> Result<GlobalConfig> {
        if !self.path.exists() {
            return Ok(GlobalConfig::default_config());
        }
        read_versioned(&self.path, "global").with_context(|| {
            format!(
                "Could not load {}. Run `sfumato init user --force` to create a v0.2 configuration.",
                self.path.display()
            )
        })
    }

    fn save(&self, config: &GlobalConfig) -> Result<()> {
        write_toml(&self.path, config)
    }
}

/// Filesystem-backed project registry and configuration repository.
#[derive(Clone, Debug)]
pub struct FilesystemProjectRepository {
    registry_path: PathBuf,
}

impl FilesystemProjectRepository {
    pub fn new(registry_path: PathBuf) -> Self {
        Self { registry_path }
    }

    pub fn default_path() -> Result<Self> {
        Ok(Self::new(ConfigPaths::discover()?.project_registry))
    }
}

impl ProjectRepository for FilesystemProjectRepository {
    fn registry(&self) -> Result<ProjectRegistry> {
        if !self.registry_path.exists() {
            return Ok(ProjectRegistry::default());
        }
        read_versioned(&self.registry_path, "project registry")
    }

    fn list(&self) -> Result<Vec<(String, RegisteredProject, bool)>> {
        let registry = self.registry()?;
        Ok(registry
            .projects
            .into_iter()
            .map(|(name, project)| {
                let active = registry.active.as_deref() == Some(&name);
                (name, project, active)
            })
            .collect())
    }

    fn load(&self, name: Option<&str>) -> Result<ProjectConfig> {
        let registry = self.registry()?;
        let (_, root) = registry.selected(name)?;
        read_versioned(&project_config_path(&root), "project")
    }

    fn save(&self, project: &ProjectConfig) -> Result<()> {
        let registry = self.registry()?;
        let registered = registry
            .projects
            .get(&project.name)
            .with_context(|| format!("Project '{}' is not registered", project.name))?;
        write_toml(&project_config_path(&registered.path), project)
    }

    fn register(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig> {
        let mut registry = self.registry()?;
        if registry.projects.contains_key(&name) {
            bail!("Project '{name}' is already registered");
        }
        validate_project_name(&name)?;
        let root = absolute_path(&path)?;
        fs::create_dir_all(&root)
            .with_context(|| format!("Could not create project root {}", root.display()))?;
        let config_path = project_config_path(&root);
        if config_path.exists() {
            bail!("Project config already exists at {}", config_path.display());
        }
        let project = ProjectConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            name: name.clone(),
            theme: DEFAULT_THEME.to_string(),
            publish_dir: None,
            model_defaults: Default::default(),
            model_roles: Default::default(),
            marp: None,
        };
        write_toml(&config_path, &project)?;
        registry
            .projects
            .insert(name.clone(), RegisteredProject { path: root });
        if activate || registry.active.is_none() {
            registry.active = Some(name);
        }
        write_toml(&self.registry_path, &registry)?;
        Ok(project)
    }

    fn set_active(&self, name: &str) -> Result<String> {
        let mut registry = self.registry()?;
        if !registry.projects.contains_key(name) {
            bail!("Project '{name}' is not registered");
        }
        registry.active = Some(name.to_string());
        write_toml(&self.registry_path, &registry)?;
        Ok(name.to_string())
    }

    fn remove(&self, name: &str) -> Result<ProjectConfig> {
        let mut registry = self.registry()?;
        let registered = registry
            .projects
            .remove(name)
            .with_context(|| format!("Project '{name}' is not registered"))?;
        let project = read_versioned(&project_config_path(&registered.path), "project")?;
        if registry.active.as_deref() == Some(name) {
            registry.active = None;
        }
        write_toml(&self.registry_path, &registry)?;
        Ok(project)
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("Could not read current directory")?
            .join(path))
    }
}
