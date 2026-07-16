use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ArtifactId, JobId, RevisionId};

/// The current schema version emitted by [`ArtifactManifest::new`].
pub const ARTIFACT_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The semantic kind of an artifact, independent of its storage location.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// Original source material supplied to a job.
    Source,
    /// A structured presentation deck.
    Deck,
    /// A Markdown document.
    Markdown,
    /// A rendered HTML document.
    Html,
    /// A rendered PDF document.
    Pdf,
    /// A raster or vector image.
    Image,
    /// A video asset.
    Video,
    /// An audio asset.
    Audio,
    /// Structured machine-readable data.
    Data,
}

impl ArtifactKind {
    /// Returns the stable string representation used in manifests.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Deck => "deck",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Data => "data",
        }
    }
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ArtifactKind {
    type Err = ArtifactManifestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "source" => Ok(Self::Source),
            "deck" => Ok(Self::Deck),
            "markdown" => Ok(Self::Markdown),
            "html" => Ok(Self::Html),
            "pdf" => Ok(Self::Pdf),
            "image" => Ok(Self::Image),
            "video" => Ok(Self::Video),
            "audio" => Ok(Self::Audio),
            "data" => Ok(Self::Data),
            _ => Err(ArtifactManifestError::UnknownKind(value.to_owned())),
        }
    }
}

/// Extensible string metadata carried by an artifact manifest.
///
/// Keys are non-empty printable names. Values may contain whitespace but not
/// control characters, making the map safe to pass between DTO boundaries.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ArtifactMetadata(BTreeMap<String, String>);

impl ArtifactMetadata {
    /// Creates empty metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds metadata from a map after validating all entries.
    pub fn try_from_map(values: BTreeMap<String, String>) -> Result<Self, ArtifactManifestError> {
        let metadata = Self(values);
        metadata.validate()?;
        Ok(metadata)
    }

    /// Inserts or replaces one metadata entry.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Option<String>, ArtifactManifestError> {
        let key = key.into();
        let value = value.into();
        validate_metadata_entry(&key, &value)?;
        Ok(self.0.insert(key, value))
    }

    /// Gets a metadata value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// Iterates over metadata in stable key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Returns whether the metadata map is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of metadata entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Validates every metadata entry.
    pub fn validate(&self) -> Result<(), ArtifactManifestError> {
        for (key, value) in &self.0 {
            validate_metadata_entry(key, value)?;
        }
        Ok(())
    }
}

/// Serializable metadata describing a job artifact without a storage path.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Stable artifact identity.
    pub artifact_id: ArtifactId,
    /// Job that produced or imported the artifact.
    pub job_id: JobId,
    /// Immutable content revision represented by the artifact.
    pub revision_id: RevisionId,
    /// Semantic artifact kind.
    pub kind: ArtifactKind,
    /// Optional media type such as `application/pdf`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Extensible, deterministic metadata.
    #[serde(default, skip_serializing_if = "ArtifactMetadata::is_empty")]
    pub metadata: ArtifactMetadata,
}

impl ArtifactManifest {
    /// Creates a manifest at the current schema version.
    pub fn new(
        artifact_id: ArtifactId,
        job_id: JobId,
        revision_id: RevisionId,
        kind: ArtifactKind,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_MANIFEST_SCHEMA_VERSION,
            artifact_id,
            job_id,
            revision_id,
            kind,
            media_type: None,
            metadata: ArtifactMetadata::new(),
        }
    }

    /// Sets and validates an optional media type.
    pub fn with_media_type(
        mut self,
        media_type: impl Into<String>,
    ) -> Result<Self, ArtifactManifestError> {
        let media_type = media_type.into();
        validate_media_type(&media_type)?;
        self.media_type = Some(media_type);
        Ok(self)
    }

    /// Validates the schema version and all free-form manifest metadata.
    pub fn validate(&self) -> Result<(), ArtifactManifestError> {
        if self.schema_version != ARTIFACT_MANIFEST_SCHEMA_VERSION {
            return Err(ArtifactManifestError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        if let Some(media_type) = &self.media_type {
            validate_media_type(media_type)?;
        }
        self.metadata.validate()
    }
}

/// An error returned by artifact kind or manifest validation.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ArtifactManifestError {
    /// The manifest uses a schema version this crate does not understand.
    #[error("unsupported artifact manifest schema version {0}")]
    UnsupportedSchemaVersion(u32),
    /// An artifact kind string is unknown.
    #[error("unknown artifact kind `{0}`")]
    UnknownKind(String),
    /// A media type is empty or malformed.
    #[error("invalid artifact media type `{0}`")]
    InvalidMediaType(String),
    /// A metadata key is empty or contains non-printable characters.
    #[error("invalid artifact metadata key `{0}`")]
    InvalidMetadataKey(String),
    /// A metadata value contains control characters.
    #[error("artifact metadata value for `{key}` contains control characters")]
    InvalidMetadataValue {
        /// The key whose value was rejected.
        key: String,
    },
}

fn validate_media_type(value: &str) -> Result<(), ArtifactManifestError> {
    let Some((kind, subtype)) = value.split_once('/') else {
        return Err(ArtifactManifestError::InvalidMediaType(value.to_owned()));
    };
    if kind.is_empty()
        || subtype.is_empty()
        || value.chars().any(|character| {
            character.is_control() || character.is_whitespace() || !character.is_ascii()
        })
    {
        return Err(ArtifactManifestError::InvalidMediaType(value.to_owned()));
    }
    Ok(())
}

fn validate_metadata_entry(key: &str, value: &str) -> Result<(), ArtifactManifestError> {
    if key.is_empty()
        || key.len() > 128
        || key
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(ArtifactManifestError::InvalidMetadataKey(key.to_owned()));
    }
    if value.chars().any(char::is_control) {
        return Err(ArtifactManifestError::InvalidMetadataValue {
            key: key.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/artifact.rs"]
mod tests;
