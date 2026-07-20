use std::fs;

use serde::{Deserialize, Serialize};
use sfumato_adapters::config_files::{
    edit_toml, read_versioned, read_versioned_snapshot, write_toml, write_toml_if_revision,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TestConfig {
    schema_version: u32,
    name: String,
}

#[test]
fn rejects_legacy_project_config_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let project_path = temp.path().join("project.toml");
    let legacy = "schema_version = 2\nname = \"demo\"\ntheme = \"gruvbox\"\n";
    fs::write(&project_path, legacy).unwrap();

    let error = read_versioned::<toml::Value>(&project_path, "project").unwrap_err();

    assert!(format!("{error:#}").contains("schema 2"));
    assert_eq!(fs::read_to_string(project_path).unwrap(), legacy);
}

#[test]
fn rejects_future_global_config_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let future = "schema_version = 99\n[user]\nlearning_style = []\n";
    fs::write(&path, future).unwrap();

    let error = read_versioned::<toml::Value>(&path, "global").unwrap_err();

    assert!(format!("{error:#}").contains("schema 99"));
    assert_eq!(fs::read_to_string(path).unwrap(), future);
}

#[test]
fn schema_reads_are_side_effect_free() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(&path, "schema_version = 5\n").unwrap();

    read_versioned::<toml::Value>(&path, "global").unwrap();

    assert!(!temp.path().join("config.toml.lock").exists());
}

#[test]
fn migrates_v4_project_plugins_to_v5_page_defaults_with_backup() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("project.toml");
    let legacy = "schema_version = 4\nname = \"demo\"\ntheme = \"gruvbox\"\nplugins = [\"shadcn\", \"motion\", \"materialui\"]\n";
    fs::write(&path, legacy).unwrap();

    let value = read_versioned::<toml::Value>(&path, "project").unwrap();

    assert_eq!(value["schema_version"].as_integer(), Some(5));
    assert_eq!(value["page"]["ui"].as_str(), Some("materialui"));
    assert_eq!(value["page"]["plugins"][0].as_str(), Some("motion"));
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("page UI 'shadcn' was replaced by 'materialui'")
    );
    assert_eq!(
        fs::read_to_string(path.with_extension("toml.v4.bak")).unwrap(),
        legacy
    );
}

#[test]
fn concurrent_edit_transactions_preserve_every_change() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(&path, "seed = 1\n").unwrap();

    std::thread::scope(|scope| {
        for index in 0..8 {
            let path = path.clone();
            scope.spawn(move || {
                edit_toml(&path, |table| {
                    table.insert(format!("worker_{index}"), toml::Value::Integer(index));
                    Ok(())
                })
                .unwrap();
            });
        }
    });

    let table = fs::read_to_string(path)
        .unwrap()
        .parse::<toml::Table>()
        .unwrap();
    for index in 0..8 {
        assert_eq!(table[&format!("worker_{index}")].as_integer(), Some(index));
    }
}

#[test]
fn stale_revision_cannot_overwrite_a_newer_config() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let original = TestConfig {
        schema_version: 4,
        name: "original".to_string(),
    };
    write_toml(&path, &original).unwrap();
    let snapshot = read_versioned_snapshot::<TestConfig>(&path, "global").unwrap();

    let mut newer = original.clone();
    newer.name = "newer writer".to_string();
    write_toml(&path, &newer).unwrap();

    let error = write_toml_if_revision(&path, &snapshot.value, &snapshot.revision).unwrap_err();
    assert!(error.to_string().contains("changed since it was loaded"));
    assert_eq!(
        read_versioned::<TestConfig>(&path, "global").unwrap().name,
        "newer writer"
    );
}
