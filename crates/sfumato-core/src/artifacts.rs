//! Transactional resource artifact contracts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sfumato_domain::{ArtifactKind, JobId, RevisionId};
use thiserror::Error;

use crate::prompts::PromptProvenance;

/// Resource kinds currently persisted by Sfumato.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactResourceKind {
    /// A Marp slide deck and its rendered sidecars.
    Slides,
}

impl ArtifactResourceKind {
    /// Stable directory and manifest name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Slides => "slides",
        }
    }
}

/// One file in a committed resource revision.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResourceArtifactFile {
    /// Path relative to the revision root.
    pub path: PathBuf,
    /// Semantic artifact kind.
    pub kind: ArtifactKind,
    /// Optional media type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

/// Complete provenance and file inventory for an immutable resource revision.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResourceArtifactManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Job that created this revision.
    pub job_id: JobId,
    /// Immutable revision identifier.
    pub revision_id: RevisionId,
    /// Parent revision when this run edits an existing resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_revision: Option<RevisionId>,
    /// Selected project.
    pub project: String,
    /// Resource type.
    pub resource_kind: ArtifactResourceKind,
    /// Stable resource slug.
    pub resource_id: String,
    /// Human-readable title.
    pub title: String,
    /// Files committed in this revision.
    pub files: Vec<ResourceArtifactFile>,
    /// Model-profile selections by role or capability.
    pub models: std::collections::BTreeMap<String, String>,
    /// Prompt templates used by this run.
    pub prompts: Vec<PromptProvenance>,
    /// Non-fatal workflow warnings.
    pub warnings: Vec<String>,
}

/// Paths returned after atomically committing a resource revision.
#[derive(Clone, Debug)]
pub struct CommittedArtifactRevision {
    /// Immutable revision directory.
    pub root: PathBuf,
    /// Manifest inside the revision directory.
    pub manifest_path: PathBuf,
    /// Current-pointer file for this resource.
    pub current_path: PathBuf,
}

/// A unique staging transaction owned by one generation or edit job.
pub trait ArtifactTransaction: Send {
    /// Unique job identifier.
    fn job_id(&self) -> &JobId;
    /// Unique revision identifier.
    fn revision_id(&self) -> &RevisionId;
    /// Existing active revision, if any.
    fn parent_revision(&self) -> Option<&RevisionId>;
    /// Private staging directory into which workflow adapters may write.
    fn staging_root(&self) -> &Path;
    /// Atomically commits the staging directory as an immutable revision.
    fn commit(
        self: Box<Self>,
        manifest: ResourceArtifactManifest,
    ) -> Result<CommittedArtifactRevision, ArtifactStoreError>;
}

/// Port for creating transactional artifact revisions.
pub trait ArtifactStore: Send + Sync {
    /// Returns the stable workspace root for a project without creating a job.
    fn project_root(&self, project: &str) -> Result<PathBuf, ArtifactStoreError>;
    /// Starts a unique staging transaction.
    fn begin(
        &self,
        project: &str,
        kind: ArtifactResourceKind,
    ) -> Result<Box<dyn ArtifactTransaction>, ArtifactStoreError>;
}

/// Artifact persistence failure.
#[derive(Debug, Error)]
pub enum ArtifactStoreError {
    /// Project or resource identifier is unsafe.
    #[error("invalid artifact identifier: {0}")]
    InvalidIdentifier(String),
    /// A relative artifact path escaped its transaction.
    #[error("unsafe artifact path: {0}")]
    UnsafePath(String),
    /// A declared artifact was not written.
    #[error("declared artifact does not exist: {0}")]
    MissingArtifact(String),
    /// Filesystem persistence failed.
    #[error("artifact persistence failed: {0}")]
    Persistence(String),
}
