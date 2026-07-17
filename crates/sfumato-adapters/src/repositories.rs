//! Filesystem-backed configuration repositories.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sfumato_core::{
    config::{
        GlobalConfig, ProjectConfig, ProjectRegistry, RegisteredProject, validate_project_name,
    },
    errors::{ErrorClass, ErrorCode, SfumatoError, SfumatoResult},
    repositories::{GlobalConfigRepository, ProjectRepository, RepositorySnapshot},
    themes::DEFAULT_THEME,
};

use crate::{
    config_dto::{GlobalConfigDto, ProjectConfigDto, ProjectRegistryDto},
    config_files::{
        ConfigPaths, edit_toml, project_config_path, read_versioned, read_versioned_snapshot,
        write_toml, write_toml_if_revision,
    },
};

/// Filesystem-backed global configuration repository.
#[derive(Clone, Debug)]
pub struct FilesystemGlobalConfigRepository {
    path: PathBuf,
}

impl FilesystemGlobalConfigRepository {
    /// Creates a repository at an explicit configuration path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Creates a repository at Sfumato's platform-specific user path.
    pub fn default_path() -> Result<Self> {
        Ok(Self::new(ConfigPaths::discover()?.user_config))
    }
}

impl GlobalConfigRepository for FilesystemGlobalConfigRepository {
    fn exists(&self) -> bool {
        self.path.is_file()
    }

    fn load(&self) -> SfumatoResult<GlobalConfig> {
        repository_result((|| {
            if !self.path.exists() {
                return Ok(GlobalConfig::default_config());
            }
            let persisted: GlobalConfigDto =
                read_versioned(&self.path, "global").with_context(|| {
                    format!(
                        "Could not load {}. Run `sfumato init user --force` to create a v0.2 configuration.",
                        self.path.display()
                    )
                })?;
            persisted.into_domain()
        })())
    }

    fn save(&self, config: &GlobalConfig) -> SfumatoResult<()> {
        repository_result((|| {
            config.validate()?;
            write_toml(&self.path, &GlobalConfigDto::from_domain(config))
        })())
    }

    fn load_snapshot(&self) -> SfumatoResult<RepositorySnapshot<GlobalConfig>> {
        repository_result((|| {
            if !self.path.exists() {
                return Ok(RepositorySnapshot {
                    value: GlobalConfig::default_config(),
                    revision: "missing".to_string(),
                });
            }
            let snapshot: RepositorySnapshot<GlobalConfigDto> =
                read_versioned_snapshot(&self.path, "global")?;
            Ok(RepositorySnapshot {
                value: snapshot.value.into_domain()?,
                revision: snapshot.revision,
            })
        })())
    }

    fn save_if_revision(&self, config: &GlobalConfig, expected: &str) -> SfumatoResult<String> {
        repository_result((|| {
            config.validate()?;
            write_toml_if_revision(&self.path, &GlobalConfigDto::from_domain(config), expected)
        })())
    }
}

/// Filesystem-backed project registry and configuration repository.
#[derive(Clone, Debug)]
pub struct FilesystemProjectRepository {
    registry_path: PathBuf,
}

impl FilesystemProjectRepository {
    /// Creates a repository using an explicit project-registry path.
    pub fn new(registry_path: PathBuf) -> Self {
        Self { registry_path }
    }

    /// Creates a repository at Sfumato's platform-specific registry path.
    pub fn default_path() -> Result<Self> {
        Ok(Self::new(ConfigPaths::discover()?.project_registry))
    }

    fn edit_registry<T>(&self, edit: impl FnOnce(&mut ProjectRegistry) -> Result<T>) -> Result<T> {
        let mut output = None;
        edit_toml(&self.registry_path, |table| {
            let mut registry = if table.is_empty() {
                ProjectRegistry::default()
            } else {
                let persisted: ProjectRegistryDto = toml::Value::Table(table.clone())
                    .try_into()
                    .context("Could not parse project registry")?;
                persisted.into_domain()?
            };
            output = Some(edit(&mut registry)?);
            *table = toml::Value::try_from(ProjectRegistryDto::from_domain(&registry))
                .context("Could not serialize project registry")?
                .as_table()
                .cloned()
                .context("Serialized project registry was not a TOML table")?;
            Ok(())
        })?;
        output.context("Project registry edit did not produce a result")
    }
}

impl ProjectRepository for FilesystemProjectRepository {
    fn registry(&self) -> SfumatoResult<ProjectRegistry> {
        repository_result((|| {
            if !self.registry_path.exists() {
                return Ok(ProjectRegistry::default());
            }
            let persisted: ProjectRegistryDto =
                read_versioned(&self.registry_path, "project registry")?;
            persisted.into_domain()
        })())
    }

    fn list(&self) -> SfumatoResult<Vec<(String, RegisteredProject, bool)>> {
        let registry = self.registry()?;
        Ok(registry
            .projects
            .into_iter()
            .map(|(name, project)| {
                let active = registry.active.as_deref() == Some(name.as_str());
                (name, project, active)
            })
            .collect())
    }

    fn load(&self, name: Option<&str>) -> SfumatoResult<ProjectConfig> {
        repository_result((|| {
            let registry = self.registry()?;
            let (_, root) = registry.selected(name)?;
            let persisted: ProjectConfigDto =
                read_versioned(&project_config_path(&root), "project")?;
            persisted.into_domain()
        })())
    }

    fn save(&self, project: &ProjectConfig) -> SfumatoResult<()> {
        repository_result((|| {
            project.validate()?;
            let registry = self.registry()?;
            let registered = registry
                .projects
                .get(&project.name)
                .with_context(|| format!("Project '{}' is not registered", project.name))?;
            write_toml(
                &project_config_path(&registered.path),
                &ProjectConfigDto::from_domain(project),
            )
        })())
    }

    fn load_snapshot(
        &self,
        name: Option<&str>,
    ) -> SfumatoResult<RepositorySnapshot<ProjectConfig>> {
        repository_result((|| {
            let registry = self.registry()?;
            let (_, root) = registry.selected(name)?;
            let snapshot: RepositorySnapshot<ProjectConfigDto> =
                read_versioned_snapshot(&project_config_path(&root), "project")?;
            Ok(RepositorySnapshot {
                value: snapshot.value.into_domain()?,
                revision: snapshot.revision,
            })
        })())
    }

    fn save_if_revision(&self, project: &ProjectConfig, expected: &str) -> SfumatoResult<String> {
        repository_result((|| {
            project.validate()?;
            let registry = self.registry()?;
            let registered = registry
                .projects
                .get(&project.name)
                .with_context(|| format!("Project '{}' is not registered", project.name))?;
            write_toml_if_revision(
                &project_config_path(&registered.path),
                &ProjectConfigDto::from_domain(project),
                expected,
            )
        })())
    }

    fn register(
        &self,
        name: String,
        path: PathBuf,
        activate: bool,
    ) -> SfumatoResult<ProjectConfig> {
        repository_result((|| {
            validate_project_name(&name)?;
            let root = absolute_path(&path)?;
            fs::create_dir_all(&root)
                .with_context(|| format!("Could not create project root {}", root.display()))?;
            let config_path = project_config_path(&root);
            let project = ProjectConfig {
                name: name.clone(),
                theme: DEFAULT_THEME.to_string(),
                publish_dir: None,
                model_defaults: Default::default(),
                model_roles: Default::default(),
                plugins: Vec::new(),
                marp: None,
            };
            self.edit_registry(|registry| {
                if registry.projects.contains_key(&name) {
                    bail!("Project '{name}' is already registered");
                }
                if config_path.exists() {
                    bail!("Project config already exists at {}", config_path.display());
                }
                write_toml(&config_path, &ProjectConfigDto::from_domain(&project))?;
                registry
                    .projects
                    .insert(name.clone(), RegisteredProject { path: root });
                if activate || registry.active.is_none() {
                    registry.active = Some(name);
                }
                Ok(project)
            })
        })())
    }

    fn set_active(&self, name: &str) -> SfumatoResult<String> {
        repository_result(self.edit_registry(|registry| {
            if !registry.projects.contains_key(name) {
                bail!("Project '{name}' is not registered");
            }
            registry.active = Some(name.to_string());
            Ok(name.to_string())
        }))
    }

    fn remove(&self, name: &str) -> SfumatoResult<ProjectConfig> {
        repository_result(self.edit_registry(|registry| {
            let registered = registry
                .projects
                .remove(name)
                .with_context(|| format!("Project '{name}' is not registered"))?;
            let persisted: ProjectConfigDto =
                read_versioned(&project_config_path(&registered.path), "project")?;
            let project = persisted.into_domain()?;
            if registry.active.as_deref() == Some(name) {
                registry.active = None;
            }
            Ok(project)
        }))
    }
}

fn repository_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            return error.clone();
        }
        let message = format!("{error:#}");
        let code = if message.contains("not registered") || message.contains("was not found") {
            ErrorCode::NotFound
        } else if message.contains("already registered")
            || message.contains("already exists")
            || message.contains("Invalid ")
        {
            ErrorCode::Validation
        } else {
            ErrorCode::Config
        };
        SfumatoError::new(code, ErrorClass::Permanent, message)
    })
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
