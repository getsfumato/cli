use super::*;
use crate::{
    config::{ProjectConfig, ProjectRegistry, RegisteredProject},
    repositories::{GlobalConfigRepository, ProjectRepository},
    themes::DEFAULT_THEME,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct MemoryGlobal(Mutex<GlobalConfig>);

impl GlobalConfigRepository for MemoryGlobal {
    fn exists(&self) -> bool {
        true
    }

    fn load(&self) -> crate::errors::SfumatoResult<GlobalConfig> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn save(&self, config: &GlobalConfig) -> crate::errors::SfumatoResult<()> {
        *self.0.lock().unwrap() = config.clone();
        Ok(())
    }
}

#[derive(Default)]
struct MemoryProjects {
    registry: Mutex<ProjectRegistry>,
    projects: Mutex<BTreeMap<String, ProjectConfig>>,
}

impl MemoryProjects {
    fn insert(&self, project: ProjectConfig, active: bool) {
        let name = project.name.clone();
        self.projects.lock().unwrap().insert(name.clone(), project);
        let mut registry = self.registry.lock().unwrap();
        registry.projects.insert(
            name.clone(),
            RegisteredProject {
                path: PathBuf::from(&name),
            },
        );
        if active {
            registry.active = Some(name);
        }
    }
}

impl ProjectRepository for MemoryProjects {
    fn registry(&self) -> crate::errors::SfumatoResult<ProjectRegistry> {
        Ok(self.registry.lock().unwrap().clone())
    }

    fn list(&self) -> crate::errors::SfumatoResult<Vec<(String, RegisteredProject, bool)>> {
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

    fn load(&self, name: Option<&str>) -> crate::errors::SfumatoResult<ProjectConfig> {
        let registry = self.registry()?;
        let selected = name
            .map(ToOwned::to_owned)
            .or(registry.active)
            .ok_or_else(|| crate::errors::SfumatoError::not_found("No project selected"))?;
        self.projects
            .lock()
            .unwrap()
            .get(&selected)
            .cloned()
            .ok_or_else(|| crate::errors::SfumatoError::not_found("Project not found"))
    }

    fn save(&self, project: &ProjectConfig) -> crate::errors::SfumatoResult<()> {
        self.projects
            .lock()
            .unwrap()
            .insert(project.name.clone(), project.clone());
        Ok(())
    }

    fn register(
        &self,
        name: String,
        _path: PathBuf,
        activate: bool,
    ) -> crate::errors::SfumatoResult<ProjectConfig> {
        let project = project(&name);
        self.insert(project.clone(), activate);
        Ok(project)
    }

    fn set_active(&self, name: &str) -> crate::errors::SfumatoResult<String> {
        self.registry.lock().unwrap().active = Some(name.to_string());
        Ok(name.to_string())
    }

    fn remove(&self, name: &str) -> crate::errors::SfumatoResult<ProjectConfig> {
        self.registry.lock().unwrap().projects.remove(name);
        self.projects
            .lock()
            .unwrap()
            .remove(name)
            .ok_or_else(|| crate::errors::SfumatoError::not_found("Project not found"))
    }
}

fn project(name: &str) -> ProjectConfig {
    ProjectConfig {
        name: name.to_string(),
        theme: DEFAULT_THEME.to_string(),
        publish_dir: None,
        model_defaults: Default::default(),
        model_roles: Default::default(),
        plugins: Vec::new(),
        marp: None,
    }
}

fn service() -> (ModelService, Arc<MemoryGlobal>, Arc<MemoryProjects>) {
    let global = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let projects = Arc::new(MemoryProjects::default());
    let service = ModelService::new(global.clone(), projects.clone()).unwrap();
    (service, global, projects)
}

#[test]
fn adds_lists_and_shows_connector_backed_profiles() {
    let (mut service, _, _) = service();
    service
        .add(
            "local-gemma".to_string(),
            "ollama".to_string(),
            "gemma4:e2b-mlx".to_string(),
            vec!["text".to_string(), "code".to_string()],
            vec!["temperature=0.2".to_string(), "max_tokens=8000".to_string()],
        )
        .unwrap();

    let profile = service.profile("local-gemma").unwrap();
    assert_eq!(profile.connector, "ollama");
    assert_eq!(profile.model, "gemma4:e2b-mlx");
    assert!(profile.capabilities.contains(&Capability::Text));
    assert_eq!(profile.options.text.max_tokens, Some(8000));
    assert!(
        service
            .add(
                "missing".to_string(),
                "unknown".to_string(),
                "model".to_string(),
                vec!["text".to_string()],
                vec![],
            )
            .is_err()
    );
}

#[test]
fn assigns_user_and_project_defaults_and_protects_used_profiles() {
    let (mut service, global, projects) = service();
    projects.insert(project("demo"), true);
    service
        .add(
            "local-gemma".to_string(),
            "ollama".to_string(),
            "gemma4:e2b-mlx".to_string(),
            vec!["text".to_string(), "code".to_string()],
            vec![],
        )
        .unwrap();
    service.use_default("text", "local-gemma", None).unwrap();
    assert_eq!(
        global
            .load()
            .unwrap()
            .defaults
            .0
            .get(&Capability::Text)
            .map(String::as_str),
        Some("local-gemma")
    );
    assert!(service.remove("local-gemma").is_err());

    service
        .use_default("code", "local-gemma", Some("demo"))
        .unwrap();
    assert_eq!(
        projects
            .load(Some("demo"))
            .unwrap()
            .model_defaults
            .get(&Capability::Code)
            .map(String::as_str),
        Some("local-gemma")
    );
}

#[test]
fn assigns_and_protects_reviewer_profiles() {
    let (mut service, global, _) = service();
    service
        .add(
            "local-review".to_string(),
            "ollama".to_string(),
            "gemma3:latest".to_string(),
            vec!["text".to_string()],
            vec![],
        )
        .unwrap();

    let changed = service
        .use_default("reviewer", "local-review", None)
        .unwrap();
    assert_eq!(changed.selection, ModelSelection::Role(ModelRole::Reviewer));
    assert_eq!(
        global.load().unwrap().model_roles.get(&ModelRole::Reviewer),
        Some(&"local-review".to_string())
    );
    assert!(service.remove("local-review").is_err());
    assert!(
        service
            .edit("local-review", None, None, vec!["code".to_string()], vec![],)
            .is_err()
    );
}

#[test]
fn parses_capabilities_and_typed_options() {
    assert_eq!(
        parse_capabilities(&["text".to_string(), "text".to_string()]).unwrap(),
        vec![Capability::Text]
    );
    let options = parse_options(&[
        "temperature=0.3".to_string(),
        "max_tokens=8000".to_string(),
        "quality=high".to_string(),
    ])
    .unwrap();
    assert_eq!(options.text.temperature, Some(0.3));
    assert_eq!(options.text.max_tokens, Some(8000));
    assert_eq!(options.image.quality.as_deref(), Some("high"));
    assert!(parse_options(&["unknown=value".to_string()]).is_err());
}

#[test]
fn edits_only_supplied_profile_fields_and_merges_options() {
    let (mut service, _, _) = service();
    service
        .add(
            "local-gemma".to_string(),
            "ollama".to_string(),
            "gemma4:e2b-mlx".to_string(),
            vec!["text".to_string(), "code".to_string()],
            vec!["temperature=0.4".to_string(), "max_tokens=4000".to_string()],
        )
        .unwrap();

    service
        .edit(
            "local-gemma",
            None,
            Some("gemma4:latest".to_string()),
            vec![],
            vec!["temperature=0.2".to_string()],
        )
        .unwrap();

    let profile = service.profile("local-gemma").unwrap();
    assert_eq!(profile.connector, "ollama");
    assert_eq!(profile.model, "gemma4:latest");
    assert_eq!(profile.options.text.temperature, Some(0.2));
    assert_eq!(profile.options.text.max_tokens, Some(4000));
    assert!(
        service
            .edit("local-gemma", None, None, vec![], vec![])
            .is_err()
    );
}

#[test]
fn edit_rejects_removing_a_capability_used_by_a_default() {
    let (mut service, _, _) = service();
    assert!(
        service
            .edit("local-text", None, None, vec!["code".to_string()], vec![],)
            .is_err()
    );
    assert!(
        service
            .profile("local-text")
            .unwrap()
            .capabilities
            .contains(&Capability::Text)
    );
}
