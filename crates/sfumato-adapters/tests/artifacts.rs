use std::{collections::BTreeMap, fs, path::PathBuf};

use sfumato_adapters::artifacts::FilesystemArtifactStore;
use sfumato_core::artifacts::ArtifactKind;
use sfumato_core::{
    artifacts::{
        ArtifactResourceKind, ArtifactStore, ArtifactStoreError, ResourceArtifactFile,
        ResourceArtifactManifest,
    },
    prompts::PromptProvenance,
};

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
        plugins: Vec::new(),
        runtimes: Vec::new(),
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

#[test]
fn rolls_back_committed_directory_when_current_pointer_cannot_be_written() {
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    let revision = transaction.revision_id().to_string();
    fs::write(transaction.staging_root().join("deck.md"), "# Fourier").unwrap();
    let manifest = manifest(
        transaction.as_ref(),
        vec![ResourceArtifactFile {
            path: PathBuf::from("deck.md"),
            kind: ArtifactKind::Markdown,
            media_type: None,
        }],
    );
    let resource_root = temp
        .path()
        .join("University/resources/slides/fourier-series");
    fs::create_dir_all(resource_root.join("current.json")).unwrap();

    assert!(transaction.commit(manifest).is_err());
    assert!(!resource_root.join("revisions").join(revision).exists());
}

#[test]
fn rejects_duplicate_and_reserved_manifest_paths() {
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    fs::write(transaction.staging_root().join("deck.md"), "# Fourier").unwrap();
    let file = ResourceArtifactFile {
        path: PathBuf::from("deck.md"),
        kind: ArtifactKind::Markdown,
        media_type: None,
    };
    let duplicate_manifest = manifest(transaction.as_ref(), vec![file.clone(), file]);
    let error = transaction.commit(duplicate_manifest).unwrap_err();
    assert!(error.to_string().contains("duplicate path"));

    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    fs::write(transaction.staging_root().join("manifest.json"), "reserved").unwrap();
    let reserved_manifest = manifest(
        transaction.as_ref(),
        vec![ResourceArtifactFile {
            path: PathBuf::from("manifest.json"),
            kind: ArtifactKind::Data,
            media_type: None,
        }],
    );
    let error = transaction.commit(reserved_manifest).unwrap_err();
    assert!(error.to_string().contains("reserved"));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_artifacts_even_when_the_target_is_a_file() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    let outside = temp.path().join("outside.md");
    fs::write(&outside, "outside").unwrap();
    symlink(&outside, transaction.staging_root().join("deck.md")).unwrap();

    let symlink_manifest = manifest(
        transaction.as_ref(),
        vec![ResourceArtifactFile {
            path: PathBuf::from("deck.md"),
            kind: ArtifactKind::Markdown,
            media_type: None,
        }],
    );
    let error = transaction.commit(symlink_manifest).unwrap_err();

    assert!(error.to_string().contains("unsafe artifact path"));
}

#[test]
fn a_second_concurrent_generation_is_refused_instead_of_blocking() {
    // The project lock is exclusive and held for the whole generation. It used to
    // be taken with a blocking `lock_exclusive` from async code, which parked a
    // tokio worker for minutes and left the second caller with no output at all.
    // Refusing immediately, with a sentence naming the project, is the contract.
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let _held = store
        .begin("University", ArtifactResourceKind::Slides)
        .expect("the first generation takes the lock");

    let Err(error) = store.begin("University", ArtifactResourceKind::Slides) else {
        panic!("a second generation cannot run in the same project");
    };
    assert!(
        matches!(error, ArtifactStoreError::Busy(_)),
        "a held lock is a distinct, self-resolving condition: {error:?}"
    );
    assert!(
        error.to_string().contains("University"),
        "the message has to name the project: {error}"
    );
}

#[test]
fn a_different_project_is_not_blocked_by_a_held_lock() {
    // The lock is per project, and sharpening it into a hard failure must not turn
    // it into a global one.
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let _held = store
        .begin("University", ArtifactResourceKind::Slides)
        .expect("the first project takes its own lock");
    assert!(
        store.begin("College", ArtifactResourceKind::Slides).is_ok(),
        "an unrelated project has its own lock"
    );
}

#[test]
fn sequential_transactions_commit_distinct_immutable_revisions() {
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());

    for index in 0..2 {
        let transaction = store
            .begin("University", ArtifactResourceKind::Slides)
            .unwrap();
        fs::write(
            transaction.staging_root().join("deck.md"),
            format!("# Fourier {index}"),
        )
        .unwrap();
        let manifest = manifest(
            transaction.as_ref(),
            vec![ResourceArtifactFile {
                path: PathBuf::from("deck.md"),
                kind: ArtifactKind::Markdown,
                media_type: None,
            }],
        );
        transaction.commit(manifest).unwrap();
    }

    let revisions = fs::read_dir(
        temp.path()
            .join("University/resources/slides/fourier-series/revisions"),
    )
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();
    assert_eq!(revisions.len(), 2);
    assert!(
        revisions
            .iter()
            .all(|entry| entry.path().join("manifest.json").is_file())
    );
    assert!(
        temp.path()
            .join("University/resources/slides/fourier-series/current.json")
            .is_file()
    );
}

#[test]
fn an_abandoned_staging_directory_is_swept_on_the_next_generation() {
    // `Drop` is the only other cleanup and it does not run on SIGINT, SIGTERM, or
    // a crash, so an interrupted run left its snapshots, assets, and partial
    // renders behind forever. Nothing swept `.staging`.
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let abandoned = temp.path().join("University/.staging/job-999-deadbeef");
    fs::create_dir_all(abandoned.join("snapshots")).unwrap();
    fs::write(abandoned.join("snapshots/frame-1.png"), [0_u8; 64]).unwrap();
    fs::write(abandoned.join("partial.mp4"), [0_u8; 64]).unwrap();

    let transaction = store
        .begin("University", ArtifactResourceKind::Videos)
        .expect("a killed run leaves no lock behind");

    assert!(
        !abandoned.exists(),
        "the abandoned staging directory should be gone"
    );
    assert!(
        transaction.staging_root().is_dir(),
        "sweeping must not remove the directory this generation just created"
    );
}

#[test]
fn sweeping_leaves_committed_revisions_untouched() {
    // The sweep runs inside the project root, so it has to be aimed at `.staging`
    // and nothing else: a revision is immutable published evidence.
    let temp = tempfile::tempdir().unwrap();
    let store = FilesystemArtifactStore::new(temp.path().to_path_buf());
    let transaction = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    fs::write(transaction.staging_root().join("deck.md"), "# Fourier").unwrap();
    let manifest = manifest(
        transaction.as_ref(),
        vec![ResourceArtifactFile {
            path: PathBuf::from("deck.md"),
            kind: ArtifactKind::Markdown,
            media_type: None,
        }],
    );
    let committed = transaction.commit(manifest).unwrap();
    assert!(committed.manifest_path.is_file());

    let next = store
        .begin("University", ArtifactResourceKind::Slides)
        .unwrap();
    assert!(
        committed.manifest_path.is_file(),
        "the sweep must not reach committed revisions"
    );
    assert!(committed.current_path.is_file());
    drop(next);
}
