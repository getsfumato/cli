use sfumato_adapters::repositories::{
    FilesystemGlobalConfigRepository, FilesystemProjectRepository,
};
use sfumato_core::{
    config::GlobalConfig,
    repositories::{GlobalConfigRepository, ProjectRepository},
};

#[test]
fn filesystem_global_config_repository_round_trips_config() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemGlobalConfigRepository::new(temp.path().join("config.toml"));
    let config = GlobalConfig::default_config();

    repository.save(&config).unwrap();

    let loaded = repository.load().unwrap();
    assert_eq!(loaded.models.len(), config.models.len());
    let persisted = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(persisted.starts_with("schema_version = 5\n"));
}

#[test]
fn filesystem_project_repository_registers_and_preserves_files_on_remove() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemProjectRepository::new(temp.path().join("projects.toml"));
    let project_root = temp.path().join("course");

    repository
        .register("course".to_string(), project_root.clone(), true)
        .unwrap();
    assert_eq!(repository.load(None).unwrap().name, "course");

    repository.remove("course").unwrap();
    assert!(project_root.join(".sfumato/project.toml").is_file());
}

#[test]
fn removes_a_registry_entry_whose_project_directory_is_gone() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemProjectRepository::new(temp.path().join("registry.toml"));
    let root = temp.path().join("doomed");
    repository
        .register("doomed".to_string(), root.clone(), true)
        .unwrap();

    std::fs::remove_dir_all(&root).unwrap();

    // The documented recovery used to fail for the exact state it recovers
    // from, because removal read the project config first.
    assert_eq!(repository.remove("doomed").unwrap(), "doomed");
    assert!(repository.registry().unwrap().projects.is_empty());
}

#[test]
fn a_missing_project_directory_names_the_recovery_command() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemProjectRepository::new(temp.path().join("registry.toml"));
    let root = temp.path().join("gone");
    repository
        .register("gone".to_string(), root.clone(), true)
        .unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    let error = repository.load(Some("gone")).unwrap_err().message;

    assert!(error.contains("sfumato project remove gone"), "{error}");
}

#[test]
fn removing_the_active_project_clears_the_active_selection() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemProjectRepository::new(temp.path().join("registry.toml"));
    repository
        .register("only".to_string(), temp.path().join("only"), true)
        .unwrap();

    repository.remove("only").unwrap();

    assert!(repository.registry().unwrap().active.is_none());
}
