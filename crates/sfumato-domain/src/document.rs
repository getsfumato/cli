//! A pure, structured representation of a sectioned prose document.
//!
//! Where a deck is a flat sequence of fixed boxes, a document is a *hierarchy*:
//! sections nest by heading level and the text flows across pages the renderer
//! decides. The order is still flat, because a document is read top to bottom;
//! the hierarchy lives in each section's level and is validated as a whole so a
//! reviewer cannot leave a level-4 heading directly under a level-2 one.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use json_patch::PatchOperation;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

use crate::{
    Patch, PatchReport, ReviewConstraint, ReviewError, ReviewFormat, ReviewSnapshot,
    ReviewableDocument, RevisionId, ValidationReport,
    markdown::{
        clean_heading, code_fence, fences, first_heading_with_level, image_sources, revision_for,
        validate_fenced_code_blocks, validate_node_id,
    },
    validate_rfc6902_patch,
};

/// The current schema version for [`SectionedDocument`] review snapshots.
pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// The deepest heading level a section may use.
const MAX_SECTION_LEVEL: u8 = 6;

/// A validated prose document made of hierarchical sections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionedDocument {
    schema_version: u32,
    revision: RevisionId,
    title: String,
    subtitle: Option<String>,
    preamble: String,
    order: Vec<SectionId>,
    sections: BTreeMap<SectionId, SectionDocument>,
}

/// Stable identity of one section inside a document.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SectionId(String);

impl SectionId {
    /// Creates a section ID from lowercase letters, numbers, and hyphens.
    pub fn new(value: impl Into<String>) -> Result<Self, ReviewError> {
        let value = value.into();
        validate_node_id(&value, "section").map_err(ReviewError::InvalidDeck)?;
        Ok(Self(value))
    }

    /// Returns the section ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the ID and returns its owned string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for SectionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for SectionId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<'de> Deserialize<'de> for SectionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_node_id(&value, "section").map_err(de::Error::custom)?;
        Ok(Self(value))
    }
}

/// Patchable content and derived metadata for one section.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SectionDocument {
    /// Revision derived from the section Markdown.
    pub revision: RevisionId,
    /// Heading level, from 2 for a top-level section to 6 for the deepest.
    pub level: u8,
    /// The section's heading text, with simple emphasis removed.
    pub heading: String,
    /// Complete Markdown source, starting with the section's own heading.
    pub markdown: String,
    /// Coarse element summary derived from the Markdown.
    pub elements: Vec<SectionElement>,
}

/// Coarse content element derived from section Markdown.
///
/// Deliberately not shared with a deck's element vocabulary: a document cares
/// about pull quotes and never about presenter notes, and collapsing the two
/// would describe neither medium honestly.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SectionElement {
    /// Prose without another recognized structure.
    Paragraph,
    /// A Markdown list.
    List {
        /// Number of top-level-looking list item lines.
        items: usize,
    },
    /// A fenced source-code block.
    Code {
        /// Optional fence language.
        language: Option<String>,
    },
    /// A fenced Mermaid diagram.
    Mermaid,
    /// Display or inline mathematical notation.
    Math,
    /// A Markdown table.
    Table {
        /// Approximate data-row count.
        rows: usize,
        /// Approximate column count.
        columns: usize,
    },
    /// A Markdown image.
    Image {
        /// Image source exactly as written in Markdown.
        source: String,
    },
    /// A block quotation.
    Quote,
    /// A footnote definition.
    Footnote,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PatchableDocument {
    schema_version: u32,
    revision: RevisionId,
    title: String,
    subtitle: Option<String>,
    preamble: String,
    order: Vec<SectionId>,
    sections: BTreeMap<SectionId, SectionDocument>,
}

impl SectionedDocument {
    /// Parses Markdown prose into a validated sectioned document.
    ///
    /// The source must carry exactly one level-1 heading, which becomes the
    /// title, and at least one level-2 or deeper section after it. An optional
    /// leading frontmatter block may declare `subtitle` and nothing else.
    pub fn from_markdown(markdown: &str) -> Result<Self, ReviewError> {
        let (frontmatter, body) = split_frontmatter(markdown)?;
        let subtitle = parse_subtitle(frontmatter)?;
        let headings = heading_lines(body);
        let Some(title_heading) = headings.first() else {
            return Err(ReviewError::InvalidDeck(
                "a document must open with a level-1 heading".to_owned(),
            ));
        };
        if title_heading.level != 1 {
            return Err(ReviewError::InvalidDeck(
                "a document must open with a level-1 heading".to_owned(),
            ));
        }
        if !body[..title_heading.start].trim().is_empty() {
            return Err(ReviewError::InvalidDeck(
                "a document cannot carry content before its title".to_owned(),
            ));
        }
        if headings.iter().skip(1).any(|heading| heading.level == 1) {
            return Err(ReviewError::InvalidDeck(
                "a document must carry exactly one level-1 heading".to_owned(),
            ));
        }
        let title =
            clean_heading(body[title_heading.start..title_heading.end].trim_start_matches('#'));

        let section_headings = &headings[1..];
        let preamble_end = section_headings
            .first()
            .map_or(body.len(), |heading| heading.start);
        let preamble = body[title_heading.end..preamble_end].trim().to_owned();

        let mut order = Vec::new();
        let mut sections = BTreeMap::new();
        for (index, heading) in section_headings.iter().enumerate() {
            let end = section_headings
                .get(index + 1)
                .map_or(body.len(), |next| next.start);
            let id = SectionId(format!("section-{}", index + 1));
            order.push(id.clone());
            sections.insert(
                id,
                section_from_markdown(body[heading.start..end].trim().to_owned()),
            );
        }

        let mut document = Self {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            revision: revision_for(""),
            title,
            subtitle,
            preamble,
            order,
            sections,
        };
        document.refresh_metadata();
        document.validate_document()?;
        Ok(document)
    }

    /// Returns the document schema version.
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the current document revision.
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    /// Returns the document title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the optional subtitle used by a cover page.
    pub fn subtitle(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    /// Returns the prose between the title and the first section.
    pub fn preamble(&self) -> &str {
        &self.preamble
    }

    /// Returns section IDs in reading order.
    pub fn order(&self) -> &[SectionId] {
        &self.order
    }

    /// Returns sections indexed by stable ID.
    pub fn sections(&self) -> &BTreeMap<SectionId, SectionDocument> {
        &self.sections
    }

    /// Gets a section by ID.
    pub fn section(&self, id: &SectionId) -> Option<&SectionDocument> {
        self.sections.get(id)
    }

    /// Gets a section by its one-based reading position.
    pub fn section_at(&self, position: usize) -> Option<(&SectionId, &SectionDocument)> {
        position
            .checked_sub(1)
            .and_then(|index| self.order.get(index))
            .and_then(|id| self.sections.get(id).map(|section| (id, section)))
    }

    /// Returns the number of sections.
    pub fn section_count(&self) -> usize {
        self.order.len()
    }

    /// Returns the heading outline as level and text pairs, in reading order.
    ///
    /// The assembler builds the table of contents from this rather than
    /// re-parsing the rendered Markdown, so the outline and the document cannot
    /// disagree about which sections exist.
    pub fn outline(&self) -> Vec<(u8, &str)> {
        self.order
            .iter()
            .filter_map(|id| self.sections.get(id))
            .map(|section| (section.level, section.heading.as_str()))
            .collect()
    }

    /// Replaces one section transactionally by reading position.
    ///
    /// Focused format repair returns a single rewritten section, so this keeps
    /// that path off the revision-guarded patch machinery while still validating
    /// the whole document before committing the change.
    pub fn replace_section_markdown_at(
        &mut self,
        position: usize,
        markdown: impl Into<String>,
    ) -> Result<(), ReviewError> {
        let Some(id) = position
            .checked_sub(1)
            .and_then(|index| self.order.get(index))
            .cloned()
        else {
            return invalid_patch(format!("section position `{position}` does not exist"));
        };
        let markdown = markdown.into();
        if markdown.trim().is_empty() {
            return invalid_patch("format repair returned an empty section".into());
        }
        let mut candidate = self.clone();
        candidate
            .sections
            .get_mut(&id)
            .expect("order and section map are validated")
            .markdown = markdown.trim().to_owned();
        candidate.refresh_metadata();
        candidate.validate_document()?;
        *self = candidate;
        Ok(())
    }

    /// Creates a review snapshot without requiring the trait in scope.
    pub fn snapshot(&self) -> Result<ReviewSnapshot, ReviewError> {
        <Self as ReviewableDocument>::snapshot(self)
    }

    /// Validates patch intent without applying it.
    pub fn validate_patch(&self, patch: &Patch) -> Result<(), ReviewError> {
        <Self as ReviewableDocument>::validate_patch(self, patch)
    }

    /// Applies a review patch transactionally without requiring the trait in scope.
    pub fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport, ReviewError> {
        <Self as ReviewableDocument>::apply_patch(self, patch)
    }

    /// Validates the document without requiring the trait in scope.
    pub fn validate(&self) -> Result<ValidationReport, ReviewError> {
        <Self as ReviewableDocument>::validate(self)
    }

    /// Renders Markdown without requiring the trait in scope.
    pub fn render(&self) -> Result<String, ReviewError> {
        <Self as ReviewableDocument>::render(self)
    }

    fn patchable(&self) -> PatchableDocument {
        PatchableDocument {
            schema_version: self.schema_version,
            revision: self.revision.clone(),
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            preamble: self.preamble.clone(),
            order: self.order.clone(),
            sections: self.sections.clone(),
        }
    }

    fn validate_patch_intent(&self, patch: &Patch) -> Result<(), ReviewError> {
        validate_rfc6902_patch(patch)?;
        if patch.0.is_empty() {
            return Ok(());
        }

        let mut tested_document = false;
        let mut tested_sections = HashSet::new();

        for operation in &patch.0 {
            if let PatchOperation::Test(operation) = operation {
                let path = operation.path.as_str();
                if path == "/revision" {
                    tested_document = true;
                } else if let Some(id) = section_revision_path(path) {
                    tested_sections.insert(id.to_owned());
                } else {
                    return invalid_patch(format!(
                        "reviewer may only test the document or section revision, not `{path}`"
                    ));
                }
                continue;
            }

            if !tested_document {
                return invalid_patch(
                    "reviewer patch must test `/revision` before making changes".into(),
                );
            }

            match operation {
                PatchOperation::Replace(operation) => {
                    let path = operation.path.as_str();
                    if let Some(id) = section_markdown_path(path) {
                        self.require_existing_tested_section(id, &tested_sections)?;
                    } else if path != "/subtitle" && path != "/preamble" {
                        return invalid_patch(format!(
                            "reviewer may only replace section Markdown, the subtitle, or the preamble, not `{path}`"
                        ));
                    }
                }
                PatchOperation::Add(operation) => {
                    let path = operation.path.as_str();
                    if let Some(id) = added_section_path(path) {
                        if self.sections.contains_key(&SectionId(id.to_owned())) {
                            return invalid_patch(format!(
                                "reviewer cannot add existing section `{id}`"
                            ));
                        }
                        validate_node_id(id, "section").map_err(ReviewError::InvalidPatch)?;
                    } else if !is_order_path(path, true) && path != "/subtitle" {
                        return invalid_patch(format!("reviewer cannot add content at `{path}`"));
                    }
                }
                PatchOperation::Remove(operation) => {
                    let path = operation.path.as_str();
                    if let Some(id) = added_section_path(path) {
                        self.require_existing_tested_section(id, &tested_sections)?;
                    } else if !is_order_path(path, false) && path != "/subtitle" {
                        return invalid_patch(format!(
                            "reviewer cannot remove content at `{path}`"
                        ));
                    }
                }
                PatchOperation::Move(operation) => {
                    let path = operation.path.as_str();
                    let from = operation.from.as_str();
                    if !is_order_path(path, true) || !is_order_path(from, false) {
                        return invalid_patch(
                            "reviewer may only move entries inside `/order`".into(),
                        );
                    }
                }
                PatchOperation::Copy(_) => {
                    return invalid_patch("reviewer patch operation `copy` is not allowed".into());
                }
                PatchOperation::Test(_) => unreachable!("test operations handled above"),
            }
        }
        Ok(())
    }

    fn require_existing_tested_section(
        &self,
        id: &str,
        tested_sections: &HashSet<String>,
    ) -> Result<(), ReviewError> {
        let id = SectionId(id.to_owned());
        if !self.sections.contains_key(&id) {
            return invalid_patch(format!("reviewer referenced unknown section `{id}`"));
        }
        if !tested_sections.contains(id.as_str()) {
            return invalid_patch(format!(
                "reviewer patch must test `/sections/{id}/revision` before changing that section"
            ));
        }
        Ok(())
    }

    fn refresh_metadata(&mut self) {
        for id in &self.order {
            if let Some(section) = self.sections.get_mut(id) {
                let markdown = section.markdown.trim().to_owned();
                *section = section_from_markdown(markdown);
            }
        }
        self.revision = document_revision(
            &self.title,
            self.subtitle.as_deref(),
            &self.preamble,
            &self.order,
            &self.sections,
        );
    }

    fn validate_document(&self) -> Result<ValidationReport, ReviewError> {
        if self.schema_version != DOCUMENT_SCHEMA_VERSION {
            return invalid_document(format!(
                "unsupported schema version {}",
                self.schema_version
            ));
        }
        if self.title.trim().is_empty() {
            return invalid_document("document title cannot be empty".into());
        }
        validate_single_line("document title", &self.title)?;
        if self
            .subtitle
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return invalid_document(
                "document subtitle must carry text or be absent entirely".into(),
            );
        }
        if let Some(subtitle) = &self.subtitle {
            validate_single_line("document subtitle", subtitle)?;
        }
        if self.order.is_empty() {
            return invalid_document("document must contain at least one section".into());
        }
        let unique = self.order.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.order.len() {
            return invalid_document("document order contains duplicate section IDs".into());
        }
        if self.sections.len() != self.order.len()
            || self.order.iter().any(|id| !self.sections.contains_key(id))
        {
            return invalid_document(
                "document order and section map do not contain the same sections".into(),
            );
        }
        if self
            .order
            .first()
            .is_some_and(|id| self.sections[id].level != 2)
        {
            return invalid_document(
                "the first section must be a level-2 heading so the outline has a root".into(),
            );
        }

        let mut previous_level = 1_u8;
        for id in &self.order {
            let section = &self.sections[id];
            validate_node_id(id.as_str(), "section").map_err(ReviewError::InvalidDeck)?;
            if section.markdown.trim().is_empty() {
                return invalid_document(format!("section `{id}` cannot be empty"));
            }
            validate_fenced_code_blocks(&section.markdown, id.as_str())
                .map_err(ReviewError::InvalidDeck)?;
            if !(2..=MAX_SECTION_LEVEL).contains(&section.level) {
                return invalid_document(format!(
                    "section `{id}` must use a heading level between 2 and {MAX_SECTION_LEVEL}"
                ));
            }
            // A jump deeper than one level leaves a hole in the outline, which
            // renders as a table of contents that skips a rank.
            if section.level > previous_level + 1 {
                return invalid_document(format!(
                    "section `{id}` jumps from level {previous_level} to {}; add the intermediate section or raise this one",
                    section.level
                ));
            }
            if section.heading.trim().is_empty() {
                return invalid_document(format!("section `{id}` must start with its heading"));
            }
            let expected = section_from_markdown(section.markdown.trim().to_owned());
            if section != &expected {
                return invalid_document(format!("section `{id}` contains stale derived metadata"));
            }
            previous_level = section.level;
        }
        Ok(ValidationReport::default())
    }
}

impl ReviewableDocument for SectionedDocument {
    fn snapshot(&self) -> Result<ReviewSnapshot, ReviewError> {
        self.validate_document()?;
        let document = serde_json::to_value(self.patchable())
            .map_err(|source| ReviewError::DocumentEncoding { source })?;
        Ok(ReviewSnapshot {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            format: ReviewFormat::SectionedDocument,
            revision: self.revision.clone(),
            document,
            constraints: vec![
                ReviewConstraint::Rfc6902Only,
                ReviewConstraint::TestDocumentRevision,
                ReviewConstraint::TestSectionRevision,
                ReviewConstraint::PreserveDocumentTitle,
                ReviewConstraint::PreserveMetadata,
                ReviewConstraint::PreserveHeadingHierarchy,
                ReviewConstraint::PreferSectionMarkdown,
                ReviewConstraint::StructuralChangesWhenNecessary,
            ],
        })
    }

    fn validate_patch(&self, patch: &Patch) -> Result<(), ReviewError> {
        self.validate_patch_intent(patch)
    }

    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport, ReviewError> {
        self.validate_patch_intent(patch)?;
        let original_count = self.section_count();
        let original_title = self.title.clone();
        let original_sections = self.sections.clone();
        let mut candidate_value = serde_json::to_value(self.patchable())
            .map_err(|source| ReviewError::DocumentEncoding { source })?;
        json_patch::patch(&mut candidate_value, patch)
            .map_err(|source| ReviewError::PatchApplication { source })?;
        refresh_section_metadata_json(&mut candidate_value)?;

        let patched: PatchableDocument = serde_json::from_value(candidate_value)
            .map_err(|source| ReviewError::InvalidPatchedStructure { source })?;
        let mut candidate = Self {
            schema_version: patched.schema_version,
            revision: patched.revision,
            title: patched.title,
            subtitle: patched.subtitle,
            preamble: patched.preamble,
            order: patched.order,
            sections: patched.sections,
        };
        if candidate.title != original_title {
            return invalid_patch("reviewer cannot change the document title".into());
        }
        candidate.refresh_metadata();
        candidate.validate_document()?;

        if candidate.section_count() * 10 < original_count * 7 {
            return invalid_patch(format!(
                "reviewer patch would reduce the document from {original_count} to {} sections",
                candidate.section_count()
            ));
        }

        let changed_nodes = original_sections
            .keys()
            .chain(candidate.sections.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|id| original_sections.get(*id) != candidate.sections.get(*id))
            .map(|id| id.as_str().to_owned())
            .collect();
        let operations = patch.0.len();
        *self = candidate;
        Ok(PatchReport {
            operations,
            changed_nodes,
        })
    }

    fn validate(&self) -> Result<ValidationReport, ReviewError> {
        self.validate_document()
    }

    fn render(&self) -> Result<String, ReviewError> {
        self.validate_document()?;
        let mut rendered = String::new();
        if let Some(subtitle) = &self.subtitle {
            rendered.push_str(&format!("---\nsubtitle: {subtitle}\n---\n\n"));
        }
        rendered.push_str(&format!("# {}\n", self.title));
        if !self.preamble.trim().is_empty() {
            rendered.push_str(&format!("\n{}\n", self.preamble.trim()));
        }
        for id in &self.order {
            rendered.push_str(&format!("\n{}\n", self.sections[id].markdown.trim()));
        }
        Ok(rendered)
    }
}

fn invalid_patch<T>(message: String) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidPatch(message))
}

fn invalid_document<T>(message: String) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidDeck(message))
}

/// Rejects a value that cannot survive a single line of YAML frontmatter.
///
/// `render` interpolates the subtitle into `---\nsubtitle: {value}\n---`, and
/// `/subtitle` is patchable by the reviewer, so a newline closed the block early
/// and could inject a second one — the rendered document then no longer parsed.
/// `ReviewableDocument::render` documents itself as producing a lossless source
/// representation, so this is enforced at validation rather than escaped at
/// render time: the invariant stays in one place, and a reviewer's patch fails
/// with a message naming the field instead of a parse error downstream.
fn validate_single_line(field: &str, value: &str) -> Result<(), ReviewError> {
    if let Some(character) = value.chars().find(|character| {
        *character == '\n' || *character == '\r' || (character.is_control() && *character != '\t')
    }) {
        return invalid_document(format!(
            "{field} must be a single line; it contains {}",
            match character {
                '\n' => "a line feed".to_string(),
                '\r' => "a carriage return".to_string(),
                other => format!("the control character U+{:04X}", other as u32),
            }
        ));
    }
    Ok(())
}

struct Heading {
    level: u8,
    start: usize,
    end: usize,
}

/// Locates every ATX heading line outside fenced code.
fn heading_lines(markdown: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut cursor = 0;
    let mut open: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let without_newline = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = without_newline.trim_start();
        if let Some((marker, length)) = code_fence(trimmed) {
            match open {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    open = None;
                }
                None => open = Some((marker, length)),
                _ => {}
            }
        } else if open.is_none() {
            let heading = trimmed.trim_start_matches('#');
            let level = trimmed.len() - heading.len();
            if level > 0 && level <= usize::from(MAX_SECTION_LEVEL) && heading.starts_with(' ') {
                headings.push(Heading {
                    level: u8::try_from(level).expect("level is between 1 and 6"),
                    start: cursor,
                    end: cursor + without_newline.len(),
                });
            }
        }
        cursor += line.len();
    }
    headings
}

/// Splits an optional leading frontmatter block from the document body.
fn split_frontmatter(markdown: &str) -> Result<(&str, &str), ReviewError> {
    let trimmed = markdown.trim_start_matches(['\u{feff}', '\n', '\r']);
    if !trimmed.starts_with("---") {
        return Ok(("", trimmed));
    }
    let located = fences(trimmed);
    if located.len() < 2 || located[0].start != 0 {
        return Err(ReviewError::InvalidDeck(
            "a document that opens with `---` must close its frontmatter block".to_owned(),
        ));
    }
    Ok((
        &trimmed[located[0].end..located[1].start],
        &trimmed[located[1].end..],
    ))
}

/// Reads the one frontmatter key a document may declare.
fn parse_subtitle(frontmatter: &str) -> Result<Option<String>, ReviewError> {
    let mut subtitle = None;
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(ReviewError::InvalidDeck(format!(
                "document frontmatter line `{line}` is not a `key: value` pair"
            )));
        };
        if key.trim() != "subtitle" {
            // Anything else is either a Marp directive that leaked in from the
            // deck prompts or a field nothing reads; both are worth rejecting
            // loudly rather than silently dropping.
            return Err(ReviewError::InvalidDeck(format!(
                "document frontmatter may only declare `subtitle`, not `{}`",
                key.trim()
            )));
        }
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if value.is_empty() {
            return Err(ReviewError::InvalidDeck(
                "document frontmatter `subtitle` cannot be empty".to_owned(),
            ));
        }
        subtitle = Some(value.to_owned());
    }
    Ok(subtitle)
}

fn refresh_section_metadata_json(value: &mut Value) -> Result<(), ReviewError> {
    let sections = value
        .get_mut("sections")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            ReviewError::InvalidPatch("patched document must contain a section object".into())
        })?;
    for section in sections.values_mut() {
        let object = section.as_object_mut().ok_or_else(|| {
            ReviewError::InvalidPatch("each patched section must be a JSON object".into())
        })?;
        let markdown = object
            .get("markdown")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReviewError::InvalidPatch("each patched section must contain Markdown".into())
            })?
            .trim()
            .to_owned();
        let derived = section_from_markdown(markdown);
        object.insert(
            "revision".to_owned(),
            Value::String(derived.revision.into_inner()),
        );
        object.insert("level".to_owned(), Value::from(derived.level));
        object.insert("heading".to_owned(), Value::String(derived.heading));
        object.insert(
            "elements".to_owned(),
            serde_json::to_value(derived.elements)
                .map_err(|source| ReviewError::DocumentEncoding { source })?,
        );
    }
    Ok(())
}

fn section_from_markdown(markdown: String) -> SectionDocument {
    let (level, heading) = first_heading_with_level(&markdown).unwrap_or((0, String::new()));
    SectionDocument {
        revision: revision_for(&markdown),
        level,
        heading,
        elements: summarize_elements(&markdown),
        markdown,
    }
}

fn summarize_elements(markdown: &str) -> Vec<SectionElement> {
    let mut elements = Vec::new();
    let mut open_fence: Option<(char, usize)> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let Some((marker, length)) = code_fence(trimmed) else {
            continue;
        };
        match open_fence {
            Some((open_marker, open_length)) if marker == open_marker && length >= open_length => {
                open_fence = None;
            }
            None => {
                let language = trimmed[length..].trim();
                if language.eq_ignore_ascii_case("mermaid") {
                    elements.push(SectionElement::Mermaid);
                } else {
                    elements.push(SectionElement::Code {
                        language: (!language.is_empty()).then(|| language.to_owned()),
                    });
                }
                open_fence = Some((marker, length));
            }
            _ => {}
        }
    }
    if markdown.contains("$$") || markdown.contains("\\(") || markdown.contains("\\[") {
        elements.push(SectionElement::Math);
    }
    let list_items = markdown
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ")
        })
        .count();
    if list_items > 0 {
        elements.push(SectionElement::List { items: list_items });
    }
    if markdown
        .lines()
        .any(|line| line.trim_start().starts_with("> "))
    {
        elements.push(SectionElement::Quote);
    }
    if markdown
        .lines()
        .any(|line| line.trim_start().starts_with("[^") && line.contains("]:"))
    {
        elements.push(SectionElement::Footnote);
    }
    if markdown
        .lines()
        .any(|line| line.trim_start().starts_with("| "))
    {
        let rows = markdown
            .lines()
            .filter(|line| line.trim_start().starts_with('|'))
            .count()
            .saturating_sub(1);
        let columns = markdown
            .lines()
            .find(|line| line.trim_start().starts_with('|'))
            .map(|line| line.matches('|').count().saturating_sub(1))
            .unwrap_or_default();
        elements.push(SectionElement::Table { rows, columns });
    }
    for source in image_sources(markdown) {
        elements.push(SectionElement::Image { source });
    }
    if elements.is_empty() {
        elements.push(SectionElement::Paragraph);
    }
    elements
}

fn document_revision(
    title: &str,
    subtitle: Option<&str>,
    preamble: &str,
    order: &[SectionId],
    sections: &BTreeMap<SectionId, SectionDocument>,
) -> RevisionId {
    let mut input = title.to_owned();
    input.push_str(subtitle.unwrap_or_default());
    input.push_str(preamble);
    for id in order {
        input.push_str(id.as_str());
        if let Some(section) = sections.get(id) {
            input.push_str(section.revision.as_str());
        }
    }
    revision_for(&input)
}

fn section_revision_path(path: &str) -> Option<&str> {
    path.strip_prefix("/sections/")?.strip_suffix("/revision")
}

fn section_markdown_path(path: &str) -> Option<&str> {
    path.strip_prefix("/sections/")?.strip_suffix("/markdown")
}

fn added_section_path(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/sections/")?;
    (!id.contains('/')).then_some(id)
}

fn is_order_path(path: &str, allow_append: bool) -> bool {
    path.strip_prefix("/order/")
        .is_some_and(|index| (allow_append && index == "-") || index.parse::<usize>().is_ok())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/document.rs"]
mod tests;
