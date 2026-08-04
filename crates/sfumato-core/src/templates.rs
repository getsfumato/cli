//! Reusable structural templates for generated resources.

use std::{fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    catalogs::CatalogListing,
    errors::{SfumatoError, SfumatoResult},
    sfumato_bail as bail,
};

/// Marker replaced with generated resource content.
pub const TEMPLATE_CONTENT_SLOT: &str = "<!-- SFUMATO_CONTENT -->";
/// Current reusable-template manifest schema.
pub const TEMPLATE_SCHEMA_VERSION: u32 = 1;

/// Resource family supported by a structural template.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum TemplateKind {
    /// Marp Markdown slide decks.
    Slides,
    /// Standalone HTML pages.
    Page,
    /// Paginated prose documents.
    Document,
}

impl TemplateKind {
    /// Stable identifier used by manifests and filesystem paths.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Slides => "slides",
            Self::Page => "page",
            Self::Document => "document",
        }
    }

    /// Default source filename inside a template package.
    pub const fn source_filename(self) -> &'static str {
        match self {
            Self::Slides => "template.md",
            Self::Page => "template.html",
            Self::Document => "template.md",
        }
    }
}

impl fmt::Display for TemplateKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TemplateKind {
    type Err = SfumatoError;

    fn from_str(value: &str) -> SfumatoResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "slides" => Ok(Self::Slides),
            "page" | "pages" => Ok(Self::Page),
            "document" | "documents" | "doc" | "docs" => Ok(Self::Document),
            _ => bail!("Unknown template kind '{value}'. Use slides, page, or document."),
        }
    }
}

/// Portable metadata for one template package.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationTemplateManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Stable package name.
    pub name: String,
    /// Resource family accepted by the package.
    pub kind: TemplateKind,
    /// Human-readable purpose.
    pub description: String,
    /// Relative path to the structural source.
    pub source: PathBuf,
}

/// Validated reusable template and its adapter-owned package root.
#[derive(Clone, Debug)]
pub struct GenerationTemplate {
    /// Package root.
    pub root: PathBuf,
    /// Validated manifest.
    pub manifest: GenerationTemplateManifest,
    /// UTF-8 structural source containing exactly one content marker.
    pub source: String,
}

impl GenerationTemplate {
    /// Inserts generated content into the package's single content marker.
    pub fn compose(&self, content: &str) -> SfumatoResult<String> {
        validate_template_source(&self.source)?;
        Ok(self
            .source
            .replacen(TEMPLATE_CONTENT_SLOT, content.trim(), 1))
    }
}

/// Compact template metadata for discovery commands.
#[derive(Clone, Debug, Serialize)]
pub struct GenerationTemplateSummary {
    /// Stable package name.
    pub name: String,
    /// Supported resource family.
    pub kind: TemplateKind,
    /// Human-readable purpose.
    pub description: String,
}

/// Persistence and discovery port for user-global structural templates.
pub trait GenerationTemplateCatalog: Send + Sync {
    /// Lists installed packages, optionally restricted by resource kind.
    /// Lists template packages, skipping and reporting any that cannot be read.
    ///
    /// A damaged package must not hide the healthy ones: `TEMPLATE_SCHEMA_VERSION`
    /// is rejected when it does not match, so the first time it is bumped every
    /// package a user already has would otherwise fail the listing — breaking
    /// discovery before they can see what needs migrating.
    fn list(
        &self,
        kind: Option<TemplateKind>,
    ) -> SfumatoResult<CatalogListing<GenerationTemplateSummary>>;
    /// Loads one named package and verifies its kind.
    /// Loads one template package, optionally requiring a kind.
    ///
    /// Generation requires it — a slides template must not be used for a
    /// document. Inspection does not: the manifest declares the kind, so demanding
    /// it up front means a package of unknown kind cannot be examined to find out
    /// what kind it is.
    fn load(&self, name: &str, kind: Option<TemplateKind>) -> SfumatoResult<GenerationTemplate>;
    /// Creates a package from a scaffold or an imported source file.
    fn create(
        &self,
        name: &str,
        kind: TemplateKind,
        source: Option<PathBuf>,
    ) -> SfumatoResult<GenerationTemplate>;
}

/// Validates the portable template package name.
pub fn validate_template_name(name: &str) -> SfumatoResult<()> {
    if name.is_empty()
        || name.starts_with('-')
        || name.ends_with('-')
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        bail!("Invalid template name '{name}'. Use lowercase letters, numbers, and hyphens.");
    }
    Ok(())
}

/// Requires exactly one structural content marker.
pub fn validate_template_source(source: &str) -> SfumatoResult<()> {
    let count = source.matches(TEMPLATE_CONTENT_SLOT).count();
    if count != 1 {
        bail!(
            "Generation template must contain exactly one {TEMPLATE_CONTENT_SLOT}; found {count}"
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/templates.rs"]
mod tests;
