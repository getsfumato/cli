use std::{collections::BTreeMap, fs, path::PathBuf};

use sfumato_adapters::artifacts::FilesystemArtifactStore;
use sfumato_core::{
    artifacts::{
        ArtifactResourceKind, ArtifactStore, ResourceArtifactFile, ResourceArtifactManifest,
    },
    prompts::PromptProvenance,
};
use sfumato_domain::ArtifactKind;

fn manifest(
    transaction: &dyn sfumato_core::artifacts::ArtifactTransaction,
    files: Vec<ResourceArtifactFile>,
) -> ResourceArtifactManifest {
    ResourceArtifactManifest {
        schema_version: 1,
        job_id: transaction.job_id().clone(),
        revision_id: transaction.revision_id().clone(),
        parent_revision: transaction.parent_revision().cloned(),
        project: "University".to_string(),
        resource_kind: ArtifactResourceKind::Slides,
        resource_id: "fourier-series".to_string(),
        title: "Fourier Series".to_string(),
        files,
        models: BTreeMap::from([("text".to_string(), "draft".to_string())]),
        prompts: Vec::<PromptProvenance>::new(),
        warnings: Vec::new(),
    }
}

#[test]
fn commits_an_immutable_revision_and_current_pointer() {
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    let deck_path = transaction.staging_root().join("deck.md");
    fs::write(&deck_path, "# Fourier Series").unwrap();
    let manifest = manifest(
        transaction.as_ref(),
        vec![ResourceArtifactFile {
            path: PathBuf::from("deck.md"),
            kind: ArtifactKind::Markdown,
            media_type: Some("text/markdown".to_string()),
        }],
    );

    let committed = transaction.commit(manifest).unwrap();

    assert!(committed.root.join("deck.md").is_file());
    assert!(committed.manifest_path.is_file());
    assert!(committed.current_path.is_file());
    assert!(
        temp.path()
            .join("University/.staging")
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn dropping_a_transaction_rolls_back_staging() {
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    let staging = transaction.staging_root().to_path_buf();
    fs::write(staging.join("partial.md"), "partial").unwrap();

    drop(transaction);

    assert!(!staging.exists());
}

#[test]
fn rejects_manifest_paths_that_escape_staging() {
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    let manifest = manifest(
        transaction.as_ref(),
        vec![ResourceArtifactFile {
            path: PathBuf::from("../outside.md"),
            kind: ArtifactKind::Markdown,
            media_type: None,
        }],
    );

    let error = transaction.commit(manifest).unwrap_err();

    assert!(error.to_string().contains("unsafe artifact path"));
}
