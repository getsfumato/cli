use super::*;

#[test]
fn initializes_switches_and_removes_projects_without_deleting_files() {
    let temp = tempfile::tempdir().unwrap();
    let registry_path = temp.path().join("projects.toml");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let service = ProjectService::new(Box::new(
        crate::repositories::FilesystemProjectRepository::new(registry_path.clone()),
    ));

    service
        .init("first".to_string(), first.clone(), true)
        .unwrap();
    service
        .init("second".to_string(), second.clone(), false)
        .unwrap();
    let registry = ProjectRegistry::load_from(&registry_path).unwrap();
    assert_eq!(registry.active.as_deref(), Some("first"));
    assert!(
        service
            .init("first".to_string(), first.clone(), true)
            .is_err()
    );

    service.use_project("second").unwrap();
    let registry = ProjectRegistry::load_from(&registry_path).unwrap();
    assert_eq!(registry.active.as_deref(), Some("second"));

    service.remove("second").unwrap();
    assert!(project_config_path(&second).exists());
    let reloaded = ProjectRegistry::load_from(&registry_path).unwrap();
    assert!(reloaded.projects.contains_key("first"));
    assert!(!reloaded.projects.contains_key("second"));
    let first_config = load_project_config(&project_config_path(&first), DEFAULT_THEME).unwrap();
    assert_eq!(first_config.theme, DEFAULT_THEME);
}
