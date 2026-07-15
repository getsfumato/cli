use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::{
    config::{
        CONFIG_SCHEMA_VERSION, GlobalConfig, ProjectConfig, ProjectRegistry, RegisteredProject,
        load_project_config, project_config_path, projects_registry_path, user_config_path,
        write_toml,
    },
    themes::{DEFAULT_THEME, ThemePackage, ThemeSummary},
};

pub trait GlobalConfigRepository {
    fn load(&self) -> Result<GlobalConfig>;
    fn save(&self, config: &GlobalConfig) -> Result<()>;
}

pub trait ProjectRepository {
    fn registry(&self) -> Result<ProjectRegistry>;
    fn list(&self) -> Result<Vec<(String, RegisteredProject, bool)>>;
    fn load(&self, name: Option<&str>) -> Result<ProjectConfig>;
    fn save(&self, project: &ProjectConfig) -> Result<()>;
    fn register(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig>;
    fn set_active(&self, name: &str) -> Result<String>;
    fn remove(&self, name: &str) -> Result<ProjectConfig>;
}

pub trait ThemeRepository {
    fn list(&self) -> Result<Vec<ThemeSummary>>;
    fn load(&self, name: &str) -> Result<ThemePackage>;
    fn create(&self, name: &str) -> Result<ThemePackage>;
    fn install_default(&self) -> Result<ThemePackage>;
}

#[derive(Clone, Debug)]
pub struct FilesystemGlobalConfigRepository {
    path: PathBuf,
}

impl FilesystemGlobalConfigRepository {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Result<Self> {
        Ok(Self::new(
            user_config_path().context("Could not find user configuration path")?,
        ))
    }
}

impl GlobalConfigRepository for FilesystemGlobalConfigRepository {
    fn load(&self) -> Result<GlobalConfig> {
        GlobalConfig::load_from(&self.path)
    }

    fn save(&self, config: &GlobalConfig) -> Result<()> {
        config.save_to(&self.path)
    }
}

#[derive(Clone, Debug)]
pub struct FilesystemProjectRepository {
    registry_path: PathBuf,
}

impl FilesystemProjectRepository {
    pub fn new(registry_path: PathBuf) -> Self {
        Self { registry_path }
    }

    pub fn default_path() -> Result<Self> {
        Ok(Self::new(
            projects_registry_path().context("Could not find project registry path")?,
        ))
    }
}

impl ProjectRepository for FilesystemProjectRepository {
    fn registry(&self) -> Result<ProjectRegistry> {
        ProjectRegistry::load_from(&self.registry_path)
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
        load_project_config(&project_config_path(&root), DEFAULT_THEME)
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
        crate::config::validate_project_name(&name)?;
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
        registry.save_to(&self.registry_path)?;
        Ok(project)
    }

    fn set_active(&self, name: &str) -> Result<String> {
        let mut registry = self.registry()?;
        if !registry.projects.contains_key(name) {
            bail!("Project '{name}' is not registered");
        }
        registry.active = Some(name.to_string());
        registry.save_to(&self.registry_path)?;
        Ok(name.to_string())
    }

    fn remove(&self, name: &str) -> Result<ProjectConfig> {
        let mut registry = self.registry()?;
        let registered = registry
            .projects
            .remove(name)
            .with_context(|| format!("Project '{name}' is not registered"))?;
        let project = load_project_config(&project_config_path(&registered.path), DEFAULT_THEME)?;
        if registry.active.as_deref() == Some(name) {
            registry.active = None;
        }
        registry.save_to(&self.registry_path)?;
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
