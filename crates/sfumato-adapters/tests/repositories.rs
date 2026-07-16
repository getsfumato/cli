use sfumato_adapters::repositories::{
    FilesystemGlobalConfigRepository, FilesystemProjectRepository,
};
use sfumato_core::{
    config::{CONFIG_SCHEMA_VERSION, GlobalConfig},
    repositories::{GlobalConfigRepository, ProjectRepository},
};

#[test]
fn filesystem_global_config_repository_round_trips_config() {
    let temp = tempfile::tempdir().unwrap();
    let repository = FilesystemGlobalConfigRepository::new(temp.path().join("config.toml"));
    let config = GlobalConfig::default_config();

    repository.save(&config).unwrap();

    let loaded = repository.load().unwrap();
    assert_eq!(loaded.schema_version, CONFIG_SCHEMA_VERSION);
    assert_eq!(loaded.models.len(), config.models.len());
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
