use std::fs;

use sfumato_adapters::config_files::read_versioned;
use sfumato_core::config::{GlobalConfig, ProjectConfig};

#[test]
fn rejects_legacy_project_config_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let project_path = temp.path().join("project.toml");
    let legacy = "schema_version = 2\nname = \"demo\"\ntheme = \"gruvbox\"\n";
    fs::write(&project_path, legacy).unwrap();

    let error = read_versioned::<ProjectConfig>(&project_path, "project").unwrap_err();

    assert!(format!("{error:#}").contains("schema 2"));
    assert_eq!(fs::read_to_string(project_path).unwrap(), legacy);
}

#[test]
fn rejects_future_global_config_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let future = "schema_version = 99\n[user]\nlearning_style = []\n";
    fs::write(&path, future).unwrap();

    let error = read_versioned::<GlobalConfig>(&path, "global").unwrap_err();

    assert!(format!("{error:#}").contains("schema 99"));
    assert_eq!(fs::read_to_string(path).unwrap(), future);
}
