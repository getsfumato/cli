use std::fs;

use sfumato_adapters::config_files::{
    edit_toml, read_versioned, read_versioned_snapshot, write_toml, write_toml_if_revision,
};
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

#[test]
fn schema_reads_are_side_effect_free() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let config = toml::to_string_pretty(&GlobalConfig::default_config()).unwrap();
    fs::write(&path, config).unwrap();

    read_versioned::<GlobalConfig>(&path, "global").unwrap();

    assert!(!temp.path().join("config.toml.lock").exists());
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
    let original = GlobalConfig::default_config();
    write_toml(&path, &original).unwrap();
    let snapshot = read_versioned_snapshot::<GlobalConfig>(&path, "global").unwrap();

    let mut newer = original.clone();
    newer.user.name = Some("newer writer".to_string());
    write_toml(&path, &newer).unwrap();

    let error = write_toml_if_revision(&path, &snapshot.value, &snapshot.revision).unwrap_err();
    assert!(error.to_string().contains("changed since it was loaded"));
    assert_eq!(
        read_versioned::<GlobalConfig>(&path, "global")
            .unwrap()
            .user
            .name
            .as_deref(),
        Some("newer writer")
    );
}
