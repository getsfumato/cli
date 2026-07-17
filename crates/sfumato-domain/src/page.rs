use std::collections::BTreeSet;

use json_patch::PatchOperation;
use serde::{Deserialize, Serialize};

use crate::{
    Patch, PatchReport, ReviewConstraint, ReviewError, ReviewFormat, ReviewSnapshot,
    ReviewableDocument, RevisionId, ValidationReport, validate_rfc6902_patch,
};

/// Current schema version for a reviewable HTML page document.
pub const PAGE_SCHEMA_VERSION: u32 = 1;

const MAX_TITLE_CHARS: usize = 200;
const MAX_BODY_CHARS: usize = 512_000;
const MAX_CSS_CHARS: usize = 256_000;
const MAX_JAVASCRIPT_CHARS: usize = 512_000;

/// Structured, revision-guarded content used to compile one standalone page.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PageDocument {
    schema_version: u32,
    revision: RevisionId,
    title: String,
    body_html: String,
    css: String,
    javascript: String,
}

impl PageDocument {
    /// Creates and validates a page from model-generated content fragments.
    pub fn new(
        title: impl Into<String>,
        body_html: impl Into<String>,
        css: impl Into<String>,
        javascript: impl Into<String>,
    ) -> Result<Self, ReviewError> {
        let mut page = Self {
            schema_version: PAGE_SCHEMA_VERSION,
            revision: revision_for("empty"),
            title: title.into().trim().to_owned(),
            body_html: body_html.into().trim().to_owned(),
            css: css.into().trim().to_owned(),
            javascript: javascript.into().trim().to_owned(),
        };
        page.refresh_revision();
        page.validate_document()?;
        Ok(page)
    }

    /// Returns the current optimistic-concurrency revision.
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    /// Returns the human-readable page title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the generated HTML body fragment.
    pub fn body_html(&self) -> &str {
        &self.body_html
    }

    /// Returns page-specific CSS.
    pub fn css(&self) -> &str {
        &self.css
    }

    /// Returns page-specific classic JavaScript.
    pub fn javascript(&self) -> &str {
        &self.javascript
    }

    /// Validates a browser-repair patch, which cannot change the title.
    pub fn validate_browser_repair_patch(&self, patch: &Patch) -> Result<(), ReviewError> {
        self.validate_patch_intent(patch, false)
    }

    /// Applies a browser-repair patch transactionally without allowing title changes.
    pub fn apply_browser_repair_patch(
        &mut self,
        patch: &Patch,
    ) -> Result<PatchReport, ReviewError> {
        self.apply_patch_with_title_policy(patch, false)
    }

    fn validate_document(&self) -> Result<ValidationReport, ReviewError> {
        if self.schema_version != PAGE_SCHEMA_VERSION {
            return invalid_page(format!(
                "unsupported schema version {}",
                self.schema_version
            ));
        }
        validate_text("title", &self.title, MAX_TITLE_CHARS, true)?;
        validate_text("body_html", &self.body_html, MAX_BODY_CHARS, true)?;
        validate_text("css", &self.css, MAX_CSS_CHARS, false)?;
        validate_text("javascript", &self.javascript, MAX_JAVASCRIPT_CHARS, false)?;
        let expected = page_revision(&self.title, &self.body_html, &self.css, &self.javascript);
        if self.revision != expected {
            return invalid_page("page contains a stale revision".into());
        }
        Ok(ValidationReport::default())
    }

    fn validate_patch_intent(&self, patch: &Patch, allow_title: bool) -> Result<(), ReviewError> {
        validate_rfc6902_patch(patch)?;
        let mut tested_revision = false;
        for operation in &patch.0 {
            if let PatchOperation::Test(test) = operation {
                if test.path.as_str() != "/revision" {
                    return invalid_patch(format!(
                        "reviewer may only test `/revision`, not `{}`",
                        test.path
                    ));
                }
                tested_revision = true;
                continue;
            }
            if !tested_revision {
                return invalid_patch(
                    "reviewer patch must test `/revision` before making changes".into(),
                );
            }
            let PatchOperation::Replace(replace) = operation else {
                return invalid_patch(
                    "page review only accepts `test` and `replace` operations".into(),
                );
            };
            let path = replace.path.as_str();
            let allowed = matches!(path, "/body_html" | "/css" | "/javascript")
                || (allow_title && path == "/title");
            if !allowed {
                return invalid_patch(format!("reviewer cannot replace page field `{path}`"));
            }
        }
        Ok(())
    }

    fn apply_patch_with_title_policy(
        &mut self,
        patch: &Patch,
        allow_title: bool,
    ) -> Result<PatchReport, ReviewError> {
        self.validate_patch_intent(patch, allow_title)?;
        let original = self.clone();
        let mut value = serde_json::to_value(&original)
            .map_err(|source| ReviewError::DocumentEncoding { source })?;
        json_patch::patch(&mut value, patch)
            .map_err(|source| ReviewError::PatchApplication { source })?;
        let mut candidate: Self = serde_json::from_value(value)
            .map_err(|source| ReviewError::InvalidPatchedPage { source })?;
        candidate.refresh_revision();
        candidate.validate_document()?;
        let changed_nodes = ["title", "body_html", "css", "javascript"]
            .into_iter()
            .filter(|field| match *field {
                "title" => original.title != candidate.title,
                "body_html" => original.body_html != candidate.body_html,
                "css" => original.css != candidate.css,
                "javascript" => original.javascript != candidate.javascript,
                _ => false,
            })
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let operations = patch.0.len();
        *self = candidate;
        Ok(PatchReport {
            operations,
            changed_nodes,
        })
    }

    fn refresh_revision(&mut self) {
        self.revision = page_revision(&self.title, &self.body_html, &self.css, &self.javascript);
    }
}

impl ReviewableDocument for PageDocument {
    fn snapshot(&self) -> Result<ReviewSnapshot, ReviewError> {
        self.validate_document()?;
        Ok(ReviewSnapshot {
            schema_version: PAGE_SCHEMA_VERSION,
            format: ReviewFormat::Html,
            revision: self.revision.clone(),
            document: serde_json::to_value(self)
                .map_err(|source| ReviewError::DocumentEncoding { source })?,
            constraints: vec![
                ReviewConstraint::Rfc6902Only,
                ReviewConstraint::TestDocumentRevision,
                ReviewConstraint::PreserveMetadata,
                ReviewConstraint::PageFieldsOnly,
            ],
        })
    }

    fn validate_patch(&self, patch: &Patch) -> Result<(), ReviewError> {
        self.validate_patch_intent(patch, true)
    }

    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport, ReviewError> {
        self.apply_patch_with_title_policy(patch, true)
    }

    fn validate(&self) -> Result<ValidationReport, ReviewError> {
        self.validate_document()
    }

    fn render(&self) -> Result<String, ReviewError> {
        self.validate_document()?;
        serde_json::to_string_pretty(self)
            .map_err(|source| ReviewError::DocumentEncoding { source })
    }
}

fn validate_text(
    field: &str,
    value: &str,
    maximum: usize,
    required: bool,
) -> Result<(), ReviewError> {
    if required && value.trim().is_empty() {
        return invalid_page(format!("{field} cannot be empty"));
    }
    if value.chars().count() > maximum {
        return invalid_page(format!(
            "{field} exceeds the maximum of {maximum} characters"
        ));
    }
    if value.chars().any(|character| character == '\0') {
        return invalid_page(format!("{field} contains a null character"));
    }
    Ok(())
}

fn page_revision(title: &str, body_html: &str, css: &str, javascript: &str) -> RevisionId {
    revision_for(&format!("{title}\0{body_html}\0{css}\0{javascript}"))
}

fn revision_for(value: &str) -> RevisionId {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    RevisionId::new(format!("{hash:016x}")).expect("hex page revision is a valid domain token")
}

fn invalid_patch<T>(message: String) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidPatch(message))
}

fn invalid_page<T>(message: String) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidPage(message))
}

#[cfg(test)]
#[path = "../tests/unit/page.rs"]
mod tests;
