use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use json_patch::{Patch, PatchOperation};

/// Maximum number of operations accepted in one review patch.
pub const MAX_PATCH_OPERATIONS: usize = 32;

/// The document representation supplied to a reviewer.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFormat {
    /// A structured presentation deck.
    SlideDeck,
    /// An HTML document.
    Html,
    /// A Python source document.
    Python,
    /// Unstructured plain text.
    PlainText,
}

/// Machine-readable mutation policies attached to a review snapshot.
///
/// Model-facing explanations of these policies belong to prompt templates;
/// these values let workflows and clients reason about the contract without
/// embedding prose in the domain model.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewConstraint {
    /// The response must be an RFC 6902 patch array.
    Rfc6902Only,
    /// A mutation must first test the deck revision.
    TestDeckRevision,
    /// A slide mutation must first test that slide's revision.
    TestSlideRevision,
    /// The title slide cannot be changed.
    PreserveTitleSlide,
    /// Derived and structural metadata cannot be changed directly.
    PreserveMetadata,
    /// Replacing slide Markdown is the preferred mutation.
    PreferSlideMarkdown,
    /// Structural slide changes are allowed only when necessary.
    StructuralChangesWhenNecessary,
    /// Only existing slide Markdown may be replaced.
    ReplaceSlideMarkdownOnly,
    /// A mutation must first test the document revision.
    TestDocumentRevision,
    /// Only the declared page content fields may be replaced.
    PageFieldsOnly,
}

/// A serializable, immutable view of a document that a reviewer may patch.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ReviewSnapshot {
    /// Schema version of the review document.
    pub schema_version: u32,
    /// Kind of document represented by [`Self::document`].
    pub format: ReviewFormat,
    /// Revision that patches must test before mutating the document.
    pub revision: crate::RevisionId,
    /// JSON representation used as the RFC 6902 patch target.
    pub document: Value,
    /// Typed mutation policies enforced by the document and workflow.
    pub constraints: Vec<ReviewConstraint>,
}

/// Summary of a successfully applied patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchReport {
    /// Number of RFC 6902 operations, including revision tests.
    pub operations: usize,
    /// Stable identifiers of nodes that changed, in sorted order.
    pub changed_nodes: Vec<String>,
}

/// Result of validating a reviewable document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    /// Non-fatal observations about otherwise valid content.
    pub warnings: Vec<String>,
}

/// Typed failures produced by review parsing, validation, and patching.
#[derive(Debug, Error)]
pub enum ReviewError {
    /// A reviewer response was not an RFC 6902 JSON Patch array.
    #[error(
        "reviewer response must be an RFC 6902 JSON Patch array, for example \
         [{{\"op\":\"test\",...}}]: {source}"
    )]
    PatchParse {
        /// JSON parser or DTO error.
        #[source]
        source: serde_json::Error,
    },
    /// A patch exceeds the operation limit.
    #[error("reviewer patch exceeds the maximum of {maximum} operations")]
    TooManyOperations {
        /// Actual operation count.
        actual: usize,
        /// Maximum accepted operation count.
        maximum: usize,
    },
    /// A syntactically valid patch violates document mutation policy.
    #[error("invalid reviewer patch: {0}")]
    InvalidPatch(String),
    /// A deck violates a structural or content invariant.
    #[error("invalid deck: {0}")]
    InvalidDeck(String),
    /// A page violates a structural or content invariant.
    #[error("invalid page: {0}")]
    InvalidPage(String),
    /// JSON serialization of a review DTO failed.
    #[error("could not encode review document: {source}")]
    DocumentEncoding {
        /// Serialization error.
        #[source]
        source: serde_json::Error,
    },
    /// Applying a valid RFC 6902 operation failed.
    #[error("could not apply reviewer patch: {source}")]
    PatchApplication {
        /// Patch application error.
        #[source]
        source: json_patch::PatchError,
    },
    /// The patched JSON no longer has the expected DTO shape.
    #[error("reviewer patch produced an invalid deck structure: {source}")]
    InvalidPatchedStructure {
        /// DTO deserialization error.
        #[source]
        source: serde_json::Error,
    },
    /// A patch no longer has the expected page DTO shape.
    #[error("reviewer patch produced an invalid page structure: {source}")]
    InvalidPatchedPage {
        /// DTO deserialization error.
        #[source]
        source: serde_json::Error,
    },
}

/// Pure operations supported by a document that participates in review.
pub trait ReviewableDocument {
    /// Creates an immutable DTO snapshot suitable for a reviewer.
    fn snapshot(&self) -> Result<ReviewSnapshot, ReviewError>;

    /// Validates patch intent without applying the patch.
    fn validate_patch(&self, patch: &Patch) -> Result<(), ReviewError>;

    /// Applies a validated patch transactionally.
    ///
    /// On failure, the document must remain unchanged.
    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport, ReviewError>;

    /// Validates all document invariants.
    fn validate(&self) -> Result<ValidationReport, ReviewError>;

    /// Renders the document to its lossless source representation.
    fn render(&self) -> Result<String, ReviewError>;
}

/// Parses an RFC 6902 JSON Patch, accepting an optional Markdown JSON fence.
pub fn parse_json_patch(response: &str) -> Result<Patch, ReviewError> {
    let response = strip_json_fence(response.trim());
    let patch =
        serde_json::from_str(response).map_err(|source| ReviewError::PatchParse { source })?;
    validate_rfc6902_patch(&patch)?;
    Ok(patch)
}

/// Validates crate-wide RFC 6902 limits for a parsed patch.
///
/// Operation DTO shape and JSON Pointer syntax have already been validated by
/// `json-patch` deserialization. Document-specific path and concurrency rules
/// are validated by [`ReviewableDocument::validate_patch`].
pub fn validate_rfc6902_patch(patch: &Patch) -> Result<(), ReviewError> {
    if patch.0.len() > MAX_PATCH_OPERATIONS {
        return Err(ReviewError::TooManyOperations {
            actual: patch.0.len(),
            maximum: MAX_PATCH_OPERATIONS,
        });
    }
    Ok(())
}

fn strip_json_fence(value: &str) -> &str {
    let value = value
        .strip_prefix("```json")
        .or_else(|| value.strip_prefix("```JSON"))
        .or_else(|| value.strip_prefix("```"))
        .unwrap_or(value);
    value.strip_suffix("```").unwrap_or(value).trim()
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/review.rs"]
mod tests;
