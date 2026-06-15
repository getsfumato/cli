use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::config::{
    CONFIG_SCHEMA_VERSION, ProjectConfig, ProjectRegistry, RegisteredProject, load_project_config,
    project_config_path, projects_registry_path, write_toml,
};
use crate::themes::DEFAULT_THEME;

#[derive(Debug)]
pub struct ProjectService {
    registry: ProjectRegistry,
    registry_path: PathBuf,
}

impl ProjectService {
    pub fn load() -> Result<Self> {
        let registry_path =
            projects_registry_path().context("Could not find project registry path")?;
        Ok(Self {
            registry: ProjectRegistry::load_from(&registry_path)?,
            registry_path,
        })
    }

    #[cfg(test)]
    fn load_from(registry_path: PathBuf) -> Result<Self> {
        Ok(Self {
            registry: ProjectRegistry::load_from(&registry_path)?,
            registry_path,
        })
    }

    pub fn init(&mut self, name: String, path: PathBuf, activate: bool) -> Result<()> {
        if self.registry.projects.contains_key(&name) {
            bail!("Project '{name}' is already registered");
        }
        if name.trim().is_empty() {
            bail!("Project name cannot be empty");
        }

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
            output_dir: PathBuf::from("Resources/Sfumato"),
            model_defaults: Default::default(),
            marp: None,
        };
        write_toml(&config_path, &project)?;

        self.registry
            .projects
            .insert(name.clone(), RegisteredProject { path: root });
        if activate || self.registry.active.is_none() {
            self.registry.active = Some(name.clone());
        }
        self.registry.save_to(&self.registry_path)?;
        println!("Initialized project '{name}'");
        Ok(())
    }

    pub fn list(&self) {
        if self.registry.projects.is_empty() {
            println!("No registered projects.");
            return;
        }
        for (name, project) in &self.registry.projects {
            let marker = if self.registry.active.as_deref() == Some(name) {
                "*"
            } else {
                " "
            };
            println!("{marker} {name}\t{}", project.path.display());
        }
    }

    pub fn show(&self, requested: Option<&str>) -> Result<String> {
        let (_, root) = self.registry.selected(requested)?;
        let project = load_project_config(&project_config_path(&root), DEFAULT_THEME)?;
        toml::to_string_pretty(&project).context("Could not render project config")
    }

    pub fn use_project(&mut self, name: &str) -> Result<()> {
        if !self.registry.projects.contains_key(name) {
            bail!("Project '{name}' is not registered");
        }
        self.registry.active = Some(name.to_string());
        self.registry.save_to(&self.registry_path)?;
        println!("Active project: {name}");
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        if self.registry.projects.remove(name).is_none() {
            bail!("Project '{name}' is not registered");
        }
        if self.registry.active.as_deref() == Some(name) {
            self.registry.active = None;
        }
        self.registry.save_to(&self.registry_path)?;
        println!("Removed project '{name}' from the registry; project files were kept.");
        Ok(())
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

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/projects.rs"]
mod tests;
