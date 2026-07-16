//! Filesystem-backed transactional artifact storage.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use serde::Serialize;
use sfumato_core::artifacts::{
    ArtifactResourceKind, ArtifactStore, ArtifactStoreError, ArtifactTransaction,
    CommittedArtifactRevision, ResourceArtifactManifest,
};
use sfumato_domain::{ArtifactId, JobId, ProjectName, RevisionId};

/// Transactional artifact store rooted at `~/.sfumato/Projects` by default.
#[derive(Clone, Debug)]
pub struct FilesystemArtifactStore {
    root: PathBuf,
}

impl FilesystemArtifactStore {
    /// Creates a store at an explicit root.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Creates a store using the platform home directory.
    pub fn default_path() -> Result<Self, ArtifactStoreError> {
        let root = dirs::home_dir()
            .ok_or_else(|| ArtifactStoreError::Persistence("home directory unavailable".into()))?
            .join(".sfumato")
            .join("Projects");
        Ok(Self::new(root))
    }
}

impl ArtifactStore for FilesystemArtifactStore {
    fn project_root(&self, project: &str) -> Result<PathBuf, ArtifactStoreError> {
        ProjectName::new(project)
            .map_err(|error| ArtifactStoreError::InvalidIdentifier(error.to_string()))?;
        Ok(self.root.join(project))
    }

    fn begin(
        &self,
        project: &str,
        kind: ArtifactResourceKind,
    ) -> Result<Box<dyn ArtifactTransaction>, ArtifactStoreError> {
        let project_root = self.project_root(project)?;
        fs::create_dir_all(&project_root).map_err(persistence)?;
        let lock_path = project_root.join(".artifacts.lock");
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(persistence)?;
        lock.lock_exclusive().map_err(persistence)?;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| ArtifactStoreError::Persistence(error.to_string()))?
            .as_nanos();
        let job_id = JobId::new(format!("job-{}-{stamp:x}", std::process::id()))
            .map_err(|error| ArtifactStoreError::InvalidIdentifier(error.to_string()))?;
        let revision_id = RevisionId::new(format!("rev-{stamp:x}"))
            .map_err(|error| ArtifactStoreError::InvalidIdentifier(error.to_string()))?;
        let staging_root = project_root.join(".staging").join(job_id.as_str());
        fs::create_dir_all(&staging_root).map_err(persistence)?;

        Ok(Box::new(FilesystemArtifactTransaction {
            project_root,
            kind,
            job_id,
            revision_id,
            parent_revision: None,
            staging_root,
            lock,
            committed: false,
        }))
    }
}

struct FilesystemArtifactTransaction {
    project_root: PathBuf,
    kind: ArtifactResourceKind,
    job_id: JobId,
    revision_id: RevisionId,
    parent_revision: Option<RevisionId>,
    staging_root: PathBuf,
    lock: File,
    committed: bool,
}

impl ArtifactTransaction for FilesystemArtifactTransaction {
    fn job_id(&self) -> &JobId {
        &self.job_id
    }

    fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }

    fn parent_revision(&self) -> Option<&RevisionId> {
        self.parent_revision.as_ref()
    }

    fn staging_root(&self) -> &Path {
        &self.staging_root
    }

    fn commit(
        mut self: Box<Self>,
        manifest: ResourceArtifactManifest,
    ) -> Result<CommittedArtifactRevision, ArtifactStoreError> {
        validate_manifest_identity(&self, &manifest)?;
        ArtifactId::new(&manifest.resource_id)
            .map_err(|error| ArtifactStoreError::InvalidIdentifier(error.to_string()))?;
        for artifact in &manifest.files {
            let relative = safe_relative(&artifact.path)?;
            let path = self.staging_root.join(relative);
            if !path.is_file() {
                return Err(ArtifactStoreError::MissingArtifact(
                    artifact.path.display().to_string(),
                ));
            }
            reject_symlink_components(&self.staging_root, relative)?;
        }

        let manifest_path = self.staging_root.join("manifest.json");
        write_json_atomic(&manifest_path, &manifest)?;
        let resource_root = self
            .project_root
            .join("resources")
            .join(self.kind.as_str())
            .join(&manifest.resource_id);
        let revisions_root = resource_root.join("revisions");
        fs::create_dir_all(&revisions_root).map_err(persistence)?;
        let committed_root = revisions_root.join(self.revision_id.as_str());
        if committed_root.exists() {
            return Err(ArtifactStoreError::Persistence(format!(
                "revision {} already exists",
                self.revision_id
            )));
        }
        fs::rename(&self.staging_root, &committed_root).map_err(persistence)?;
        self.committed = true;

        let current_path = resource_root.join("current.json");
        write_json_atomic(
            &current_path,
            &CurrentRevision {
                schema_version: 1,
                revision_id: self.revision_id.clone(),
                manifest: PathBuf::from("revisions")
                    .join(self.revision_id.as_str())
                    .join("manifest.json"),
            },
        )?;
        FileExt::unlock(&self.lock).map_err(persistence)?;
        Ok(CommittedArtifactRevision {
            root: committed_root.clone(),
            manifest_path: committed_root.join("manifest.json"),
            current_path,
        })
    }
}

impl Drop for FilesystemArtifactTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.staging_root);
        }
        let _ = FileExt::unlock(&self.lock);
    }
}

#[derive(Serialize)]
struct CurrentRevision {
    schema_version: u32,
    revision_id: RevisionId,
    manifest: PathBuf,
}

fn validate_manifest_identity(
    transaction: &FilesystemArtifactTransaction,
    manifest: &ResourceArtifactManifest,
) -> Result<(), ArtifactStoreError> {
    if manifest.schema_version != 1
        || manifest.job_id != transaction.job_id
        || manifest.revision_id != transaction.revision_id
        || manifest.resource_kind != transaction.kind
    {
        return Err(ArtifactStoreError::Persistence(
            "artifact manifest does not match its transaction".into(),
        ));
    }
    Ok(())
}

fn safe_relative(path: &Path) -> Result<&Path, ArtifactStoreError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ArtifactStoreError::UnsafePath(path.display().to_string()));
    }
    Ok(path)
}

fn reject_symlink_components(root: &Path, relative: &Path) -> Result<(), ArtifactStoreError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(persistence)?;
        if metadata.file_type().is_symlink() {
            return Err(ArtifactStoreError::UnsafePath(
                current.display().to_string(),
            ));
        }
    }
    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), ArtifactStoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| ArtifactStoreError::Persistence("artifact path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(persistence)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(persistence)?;
    serde_json::to_writer_pretty(&mut temporary, value)
        .map_err(|error| ArtifactStoreError::Persistence(error.to_string()))?;
    temporary.write_all(b"\n").map_err(persistence)?;
    temporary.as_file().sync_all().map_err(persistence)?;
    temporary
        .persist(path)
        .map_err(|error| persistence(error.error))?;
    Ok(())
}

fn persistence(error: impl std::fmt::Display) -> ArtifactStoreError {
    ArtifactStoreError::Persistence(error.to_string())
}
