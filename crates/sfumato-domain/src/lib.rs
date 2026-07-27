#![warn(missing_docs)]
//! Pure domain types and review rules for Sfumato.
//!
//! This crate deliberately contains no filesystem, network, async runtime, or
//! process integration. Callers supply text and identifiers and receive typed
//! values, validation reports, or domain errors.

mod artifact;
mod deck;
mod page;
mod primitives;
mod review;
mod video;

pub use artifact::{
    ARTIFACT_MANIFEST_SCHEMA_VERSION, ArtifactKind, ArtifactManifest, ArtifactManifestError,
    ArtifactMetadata,
};
pub use deck::{
    DECK_SCHEMA_VERSION, DeckDocument, SlideDocument, SlideElement, SlideId, SlideKind,
};
pub use page::{PAGE_SCHEMA_VERSION, PageDocument};
pub use primitives::{
    ArtifactId, Capability, CapabilityParseError, JobId, ModelProfileName, ProjectName, RevisionId,
    SecretRef, ThemeName, ValueError,
};
pub use review::{
    MAX_PATCH_OPERATIONS, Patch, PatchOperation, PatchReport, ReviewConstraint, ReviewError,
    ReviewFormat, ReviewSnapshot, ReviewableDocument, ValidationReport, parse_json_patch,
    validate_rfc6902_patch,
};
pub use video::{
    VIDEO_PLAN_SCHEMA_VERSION, VIDEO_SOURCE_SCHEMA_VERSION, VideoEngine, VideoPlanDocument,
    VideoScene, VideoSceneProduction, VideoSourceDocument, VideoWorkflow,
};
