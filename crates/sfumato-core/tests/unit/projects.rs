use super::*;
use crate::{
    config::{ProjectRegistry, RegisteredProject},
    repositories::ProjectRepository,
    themes::DEFAULT_THEME,
};
use std::{collections::BTreeMap, sync::{Arc, Mutex}};

#[derive(Default)]
struct MemoryProjects {
    active: Mutex<Option<String>>,
    projects: Mutex<BTreeMap<String, (PathBuf, ProjectConfig)>>,
}

impl ProjectRepository for MemoryProjects {
    fn registry(&self) -> Result<ProjectRegistry> {
        let projects = self
            .projects
            .lock()
            .unwrap()
            .iter()
            .map(|(name, (path, _))| {
                (name.clone(), RegisteredProject { path: path.clone() })
            })
            .collect();
        Ok(ProjectRegistry {
            active: self.active.lock().unwrap().clone(),
            projects,
        })
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
        let selected = name
            .map(ToOwned::to_owned)
            .or_else(|| self.active.lock().unwrap().clone())
            .context("No project selected")?;
        self.projects
            .lock()
            .unwrap()
            .get(&selected)
            .map(|(_, project)| project.clone())
            .context("Project not found")
    }

    fn save(&self, project: &ProjectConfig) -> Result<()> {
        let mut projects = self.projects.lock().unwrap();
        let path = projects
            .get(&project.name)
            .map(|(path, _)| path.clone())
            .context("Project not found")?;
        projects.insert(project.name.clone(), (path, project.clone()));
        Ok(())
    }

    fn register(&self, name: String, path: PathBuf, activate: bool) -> Result<ProjectConfig> {
        let mut projects = self.projects.lock().unwrap();
        if projects.contains_key(&name) {
            bail!("Project already exists");
        }
        let project = ProjectConfig {
            name: name.clone(),
            theme: DEFAULT_THEME.to_string(),
            publish_dir: None,
            model_defaults: Default::default(),
            model_roles: Default::default(),
            plugins: Vec::new(),
            marp: None,
        };
        projects.insert(name.clone(), (path, project.clone()));
        if activate || self.active.lock().unwrap().is_none() {
            *self.active.lock().unwrap() = Some(name);
        }
        Ok(project)
    }

    fn set_active(&self, name: &str) -> Result<String> {
        if !self.projects.lock().unwrap().contains_key(name) {
            bail!("Project not found");
        }
        *self.active.lock().unwrap() = Some(name.to_string());
        Ok(name.to_string())
    }

    fn remove(&self, name: &str) -> Result<ProjectConfig> {
        self.projects
            .lock()
            .unwrap()
            .remove(name)
            .map(|(_, project)| project)
            .context("Project not found")
    }
}

#[test]
fn initializes_switches_and_removes_projects() {
    let repository = Arc::new(MemoryProjects::default());
    let service = ProjectService::new(repository.clone());

    service
        .init("first".to_string(), PathBuf::from("first"), true)
        .unwrap();
    service
        .init("second".to_string(), PathBuf::from("second"), false)
        .unwrap();
    assert_eq!(repository.registry().unwrap().active.as_deref(), Some("first"));
    assert!(
        service
            .init("first".to_string(), PathBuf::from("first"), true)
            .is_err()
    );

    service.use_project("second").unwrap();
    assert_eq!(repository.registry().unwrap().active.as_deref(), Some("second"));
    service.remove("second").unwrap();
    assert!(repository.load(Some("second")).is_err());
    assert_eq!(service.show(Some("first")).unwrap().theme, DEFAULT_THEME);
}
