use std::collections::{BTreeMap, BTreeSet, HashSet};

use json_patch::PatchOperation;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

use crate::{
    Patch, PatchReport, ReviewConstraint, ReviewError, ReviewFormat, ReviewSnapshot,
    ReviewableDocument, RevisionId, ValidationReport,
    markdown::{
        code_fence as markdown_code_fence, fences as markdown_fences, first_heading,
        image_sources as markdown_image_sources, revision_for, validate_fenced_code_blocks,
        validate_node_id,
    },
    validate_rfc6902_patch,
};

/// The current schema version for [`DeckDocument`] review snapshots.
pub const DECK_SCHEMA_VERSION: u32 = 1;

/// A pure, structured representation of a Marp presentation deck.
///
/// Frontmatter is retained for rendering but intentionally excluded from the
/// patchable snapshot. Slide metadata and revisions are derived from Markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckDocument {
    schema_version: u32,
    revision: RevisionId,
    title: String,
    order: Vec<SlideId>,
    slides: BTreeMap<SlideId, SlideDocument>,
    frontmatter: String,
}

/// Stable identity of one slide inside a deck.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SlideId(String);

impl SlideId {
    /// Creates a slide ID from lowercase letters, numbers, and hyphens.
    pub fn new(value: impl Into<String>) -> Result<Self, ReviewError> {
        let value = value.into();
        validate_slide_id(&value).map_err(ReviewError::InvalidDeck)?;
        Ok(Self(value))
    }

    /// Returns the slide ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the ID and returns its owned string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for SlideId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for SlideId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<'de> Deserialize<'de> for SlideId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_slide_id(&value).map_err(de::Error::custom)?;
        Ok(Self(value))
    }
}

/// Patchable content and derived metadata for one slide.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SlideDocument {
    /// Revision derived from the slide Markdown.
    pub revision: RevisionId,
    /// Semantic role of the slide in the deck.
    pub kind: SlideKind,
    /// First Markdown heading, with simple emphasis removed.
    pub heading: Option<String>,
    /// Complete Markdown source for the slide.
    pub markdown: String,
    /// Coarse element summary derived from the Markdown.
    pub elements: Vec<SlideElement>,
}

/// Semantic role of a slide.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlideKind {
    /// The first slide, whose heading preserves the deck title.
    Title,
    /// A Marp lead slide used as a section boundary.
    Section,
    /// A regular content slide.
    #[default]
    Content,
}

/// Coarse content element derived from slide Markdown.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlideElement {
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
    /// An HTML comment, commonly used for presenter notes or Marp directives.
    PresenterNotes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PatchableDeck {
    schema_version: u32,
    revision: RevisionId,
    title: String,
    order: Vec<SlideId>,
    slides: BTreeMap<SlideId, SlideDocument>,
}

impl DeckDocument {
    /// Parses a Marp document into a validated deck.
    ///
    /// The source must contain Marp frontmatter and at least two slides. Slide
    /// separators inside fenced code blocks are preserved as slide content.
    pub fn from_marp(markdown: &str, title: &str) -> Result<Self, ReviewError> {
        let fences = markdown_fences(markdown);
        if fences.len() < 3
            || fences[0].start != 0
            || !frontmatter_contains_key(&markdown[fences[0].end..fences[1].start], "marp")
        {
            return Err(ReviewError::InvalidDeck(
                "cannot build a deck from invalid Marp Markdown".to_owned(),
            ));
        }

        let frontmatter = markdown[..fences[1].end].trim().to_owned();
        let mut fragments = Vec::new();
        let mut start = fences[1].end;
        for separator in fences.iter().skip(2) {
            fragments.push(markdown[start..separator.start].trim().to_owned());
            start = separator.end;
        }
        fragments.push(markdown[start..].trim().to_owned());

        let mut order = Vec::new();
        let mut slides = BTreeMap::new();
        for (index, fragment) in fragments.into_iter().enumerate() {
            let id = SlideId(format!("slide-{}", index + 1));
            order.push(id.clone());
            slides.insert(id, slide_from_markdown(fragment, index == 0));
        }

        let mut document = Self {
            schema_version: DECK_SCHEMA_VERSION,
            revision: revision_for(""),
            title: title.to_owned(),
            order,
            slides,
            frontmatter,
        };
        document.refresh_metadata();
        document.validate_document()?;
        Ok(document)
    }

    /// Returns the deck schema version.
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the current deck revision.
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    /// Returns the deck title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the retained Marp frontmatter.
    pub fn frontmatter(&self) -> &str {
        &self.frontmatter
    }

    /// Returns slide IDs in presentation order.
    pub fn order(&self) -> &[SlideId] {
        &self.order
    }

    /// Returns slides indexed by stable ID.
    pub fn slides(&self) -> &BTreeMap<SlideId, SlideDocument> {
        &self.slides
    }

    /// Gets a slide by ID.
    pub fn slide(&self, id: &SlideId) -> Option<&SlideDocument> {
        self.slides.get(id)
    }

    /// Gets a slide by its one-based presentation position.
    pub fn slide_at(&self, position: usize) -> Option<(&SlideId, &SlideDocument)> {
        position
            .checked_sub(1)
            .and_then(|index| self.order.get(index))
            .and_then(|id| self.slides.get(id).map(|slide| (id, slide)))
    }

    /// Replaces one non-title slide transactionally by presentation position.
    ///
    /// This focused operation is intended for deterministic layout repair after
    /// a reviewer returns a single slide fragment. Structural review and user
    /// edits continue to use revision-guarded RFC 6902 patches.
    pub fn replace_slide_markdown_at(
        &mut self,
        position: usize,
        markdown: impl Into<String>,
    ) -> Result<(), ReviewError> {
        let markdown = markdown.into();
        if !markdown_fences(&markdown).is_empty() {
            return invalid_patch(
                "a single-slide replacement cannot contain a top-level `---`".into(),
            );
        }
        self.replace_slide_fragment_at(position, markdown)
    }

    /// Replaces one non-title slide with one or more validated slide fragments.
    ///
    /// Top-level `---` separators split the focused replacement into additional
    /// slides. Separators inside fenced code remain part of the original slide.
    pub fn replace_slide_fragment_at(
        &mut self,
        position: usize,
        markdown: impl Into<String>,
    ) -> Result<(), ReviewError> {
        if position == 1 {
            return invalid_patch("the title slide cannot be replaced by layout repair".into());
        }
        let Some(id) = position
            .checked_sub(1)
            .and_then(|index| self.order.get(index))
            .cloned()
        else {
            return invalid_patch(format!("slide position `{position}` does not exist"));
        };

        let fragments = split_slide_fragment(&markdown.into());
        if fragments.iter().any(|fragment| fragment.trim().is_empty()) {
            return invalid_patch("layout repair returned an empty slide fragment".into());
        }

        let mut candidate = self.clone();
        candidate
            .slides
            .get_mut(&id)
            .expect("order and slide map are validated")
            .markdown = fragments[0].clone();
        let insertion_index = position;
        for (offset, fragment) in fragments.into_iter().skip(1).enumerate() {
            let mut suffix = offset + 2;
            let new_id = loop {
                let candidate_id = SlideId(format!("{}-part-{suffix}", id.as_str()));
                if !candidate.slides.contains_key(&candidate_id) {
                    break candidate_id;
                }
                suffix += 1;
            };
            candidate
                .slides
                .insert(new_id.clone(), slide_from_markdown(fragment, false));
            candidate.order.insert(insertion_index + offset, new_id);
        }
        candidate.refresh_metadata();
        candidate.validate_document()?;
        *self = candidate;
        Ok(())
    }

    /// Returns the number of slides.
    pub fn slide_count(&self) -> usize {
        self.order.len()
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

    /// Validates the deck without requiring the trait in scope.
    pub fn validate(&self) -> Result<ValidationReport, ReviewError> {
        <Self as ReviewableDocument>::validate(self)
    }

    /// Renders Marp Markdown without requiring the trait in scope.
    pub fn render(&self) -> Result<String, ReviewError> {
        <Self as ReviewableDocument>::render(self)
    }

    fn patchable(&self) -> PatchableDeck {
        PatchableDeck {
            schema_version: self.schema_version,
            revision: self.revision.clone(),
            title: self.title.clone(),
            order: self.order.clone(),
            slides: self.slides.clone(),
        }
    }

    fn validate_patch_intent(&self, patch: &Patch) -> Result<(), ReviewError> {
        validate_rfc6902_patch(patch)?;
        if patch.0.is_empty() {
            return Ok(());
        }

        let title_slide = self.order.first().ok_or_else(|| {
            ReviewError::InvalidDeck("deck does not contain a title slide".into())
        })?;
        let mut tested_deck = false;
        let mut tested_slides = HashSet::new();

        for operation in &patch.0 {
            if let PatchOperation::Test(operation) = operation {
                let path = operation.path.as_str();
                if path == "/revision" {
                    tested_deck = true;
                } else if let Some(id) = slide_revision_path(path) {
                    tested_slides.insert(id.to_owned());
                } else {
                    return invalid_patch(format!(
                        "reviewer may only test the deck or slide revision, not `{path}`"
                    ));
                }
                continue;
            }

            if !tested_deck {
                return invalid_patch(
                    "reviewer patch must test `/revision` before making changes".into(),
                );
            }

            match operation {
                PatchOperation::Replace(operation) => {
                    let path = operation.path.as_str();
                    let Some(id) = slide_markdown_path(path) else {
                        return invalid_patch(format!(
                            "reviewer may only replace slide Markdown, not `{path}`"
                        ));
                    };
                    self.require_existing_mutable_slide(id, title_slide, &tested_slides)?;
                }
                PatchOperation::Add(operation) => {
                    let path = operation.path.as_str();
                    if let Some(id) = added_slide_path(path) {
                        if self.slides.contains_key(&SlideId(id.to_owned())) {
                            return invalid_patch(format!(
                                "reviewer cannot add existing slide `{id}`"
                            ));
                        }
                        validate_slide_id(id).map_err(ReviewError::InvalidPatch)?;
                    } else if !is_order_path(path, true) {
                        return invalid_patch(format!("reviewer cannot add content at `{path}`"));
                    }
                }
                PatchOperation::Remove(operation) => {
                    let path = operation.path.as_str();
                    if let Some(id) = removed_slide_path(path) {
                        self.require_existing_mutable_slide(id, title_slide, &tested_slides)?;
                    } else if !is_order_path(path, false) {
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

    fn require_existing_mutable_slide(
        &self,
        id: &str,
        title_slide: &SlideId,
        tested_slides: &HashSet<String>,
    ) -> Result<(), ReviewError> {
        let id = SlideId(id.to_owned());
        if &id == title_slide {
            return invalid_patch("reviewer cannot modify or remove the title slide".into());
        }
        if !self.slides.contains_key(&id) {
            return invalid_patch(format!("reviewer referenced unknown slide `{id}`"));
        }
        if !tested_slides.contains(id.as_str()) {
            return invalid_patch(format!(
                "reviewer patch must test `/slides/{id}/revision` before changing that slide"
            ));
        }
        Ok(())
    }

    fn refresh_metadata(&mut self) {
        for (index, id) in self.order.iter().enumerate() {
            if let Some(slide) = self.slides.get_mut(id) {
                let markdown = slide.markdown.trim().to_owned();
                *slide = slide_from_markdown(markdown, index == 0);
            }
        }
        self.revision = deck_revision(&self.title, &self.order, &self.slides);
    }

    fn validate_document(&self) -> Result<ValidationReport, ReviewError> {
        if self.schema_version != DECK_SCHEMA_VERSION {
            return invalid_deck(format!(
                "unsupported schema version {}",
                self.schema_version
            ));
        }
        if self.title.trim().is_empty() {
            return invalid_deck("deck title cannot be empty".into());
        }
        if self.order.len() < 2 {
            return invalid_deck("deck must contain at least two slides".into());
        }
        let unique = self.order.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.order.len() {
            return invalid_deck("deck order contains duplicate slide IDs".into());
        }
        if self.slides.len() != self.order.len()
            || self.order.iter().any(|id| !self.slides.contains_key(id))
        {
            return invalid_deck("deck order and slide map do not contain the same slides".into());
        }
        let title_slide = self
            .slides
            .get(&self.order[0])
            .ok_or_else(|| ReviewError::InvalidDeck("title slide is missing".into()))?;
        if title_slide.kind != SlideKind::Title
            || title_slide.heading.as_deref() != Some(self.title.as_str())
        {
            return invalid_deck("title slide must preserve the deck title".into());
        }

        for (index, id) in self.order.iter().enumerate() {
            let slide = &self.slides[id];
            validate_slide_id(id.as_str()).map_err(ReviewError::InvalidDeck)?;
            if slide.markdown.trim().is_empty() {
                return invalid_deck(format!("slide `{id}` cannot be empty"));
            }
            validate_fenced_code_blocks(&slide.markdown, id.as_str())
                .map_err(ReviewError::InvalidDeck)?;
            if !markdown_fences(&slide.markdown).is_empty() {
                return invalid_deck(format!(
                    "slide `{id}` contains a top-level `---`; add a separate slide instead"
                ));
            }
            let expected = slide_from_markdown(slide.markdown.trim().to_owned(), index == 0);
            if slide != &expected {
                return invalid_deck(format!("slide `{id}` contains stale derived metadata"));
            }
        }
        Ok(ValidationReport::default())
    }
}

impl ReviewableDocument for DeckDocument {
    fn snapshot(&self) -> Result<ReviewSnapshot, ReviewError> {
        self.validate_document()?;
        let document = serde_json::to_value(self.patchable())
            .map_err(|source| ReviewError::DocumentEncoding { source })?;
        Ok(ReviewSnapshot {
            schema_version: DECK_SCHEMA_VERSION,
            format: ReviewFormat::SlideDeck,
            revision: self.revision.clone(),
            document,
            constraints: vec![
                ReviewConstraint::Rfc6902Only,
                ReviewConstraint::TestDeckRevision,
                ReviewConstraint::TestSlideRevision,
                ReviewConstraint::PreserveTitleSlide,
                ReviewConstraint::PreserveMetadata,
                ReviewConstraint::PreferSlideMarkdown,
                ReviewConstraint::StructuralChangesWhenNecessary,
            ],
        })
    }

    fn validate_patch(&self, patch: &Patch) -> Result<(), ReviewError> {
        self.validate_patch_intent(patch)
    }

    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport, ReviewError> {
        self.validate_patch_intent(patch)?;
        let original_count = self.slide_count();
        let original_title_slide = self.order.first().cloned();
        let original_slides = self.slides.clone();
        let mut candidate_value = serde_json::to_value(self.patchable())
            .map_err(|source| ReviewError::DocumentEncoding { source })?;
        json_patch::patch(&mut candidate_value, patch)
            .map_err(|source| ReviewError::PatchApplication { source })?;
        refresh_slide_metadata_json(&mut candidate_value)?;

        let patched: PatchableDeck = serde_json::from_value(candidate_value)
            .map_err(|source| ReviewError::InvalidPatchedStructure { source })?;
        let mut candidate = Self {
            schema_version: patched.schema_version,
            revision: patched.revision,
            title: patched.title,
            order: patched.order,
            slides: patched.slides,
            frontmatter: self.frontmatter.clone(),
        };
        if candidate.order.first() != original_title_slide.as_ref() {
            return invalid_patch("reviewer cannot move or replace the title slide".into());
        }
        candidate.refresh_metadata();
        candidate.validate_document()?;

        if candidate.slide_count() * 10 < original_count * 7 {
            return invalid_patch(format!(
                "reviewer patch would reduce the deck from {original_count} to {} slides",
                candidate.slide_count()
            ));
        }

        let changed_nodes = original_slides
            .keys()
            .chain(candidate.slides.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|id| original_slides.get(*id) != candidate.slides.get(*id))
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
        let body = self
            .order
            .iter()
            .map(|id| self.slides[id].markdown.trim())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Ok(format!("{}\n\n{}", self.frontmatter.trim(), body))
    }
}

fn invalid_patch<T>(message: String) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidPatch(message))
}

fn invalid_deck<T>(message: String) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidDeck(message))
}

fn refresh_slide_metadata_json(value: &mut Value) -> Result<(), ReviewError> {
    let slides = value
        .get_mut("slides")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            ReviewError::InvalidPatch("patched deck must contain a slide object".into())
        })?;
    for slide in slides.values_mut() {
        let object = slide.as_object_mut().ok_or_else(|| {
            ReviewError::InvalidPatch("each patched slide must be a JSON object".into())
        })?;
        let markdown = object
            .get("markdown")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReviewError::InvalidPatch("each patched slide must contain Markdown".into())
            })?
            .trim()
            .to_owned();
        let derived = slide_from_markdown(markdown, false);
        object.insert(
            "revision".to_owned(),
            Value::String(derived.revision.into_inner()),
        );
        object.insert(
            "kind".to_owned(),
            serde_json::to_value(derived.kind)
                .map_err(|source| ReviewError::DocumentEncoding { source })?,
        );
        object.insert(
            "heading".to_owned(),
            serde_json::to_value(derived.heading)
                .map_err(|source| ReviewError::DocumentEncoding { source })?,
        );
        object.insert(
            "elements".to_owned(),
            serde_json::to_value(derived.elements)
                .map_err(|source| ReviewError::DocumentEncoding { source })?,
        );
    }
    Ok(())
}

fn slide_from_markdown(markdown: String, title: bool) -> SlideDocument {
    let heading = first_heading(&markdown);
    let kind = if title {
        SlideKind::Title
    } else if markdown.contains("<!-- _class: lead -->") {
        SlideKind::Section
    } else {
        SlideKind::Content
    };
    SlideDocument {
        revision: revision_for(&markdown),
        kind,
        heading,
        elements: summarize_elements(&markdown),
        markdown,
    }
}

fn summarize_elements(markdown: &str) -> Vec<SlideElement> {
    let mut elements = Vec::new();
    let mut open_fence: Option<(char, usize)> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let Some((marker, length)) = markdown_code_fence(trimmed) else {
            continue;
        };
        match open_fence {
            Some((open_marker, open_length)) if marker == open_marker && length >= open_length => {
                open_fence = None;
            }
            None => {
                let language = trimmed[length..].trim();
                if language.eq_ignore_ascii_case("mermaid") {
                    elements.push(SlideElement::Mermaid);
                } else {
                    elements.push(SlideElement::Code {
                        language: (!language.is_empty()).then(|| language.to_owned()),
                    });
                }
                open_fence = Some((marker, length));
            }
            _ => {}
        }
    }
    if markdown.contains("$$") || markdown.contains("\\(") || markdown.contains("\\[") {
        elements.push(SlideElement::Math);
    }
    if markdown.contains("<!--") && markdown.contains("-->") {
        elements.push(SlideElement::PresenterNotes);
    }
    let list_items = markdown
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ")
        })
        .count();
    if list_items > 0 {
        elements.push(SlideElement::List { items: list_items });
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
        elements.push(SlideElement::Table { rows, columns });
    }
    for source in markdown_image_sources(markdown) {
        elements.push(SlideElement::Image { source });
    }
    if elements.is_empty() {
        elements.push(SlideElement::Paragraph);
    }
    elements
}

fn deck_revision(
    title: &str,
    order: &[SlideId],
    slides: &BTreeMap<SlideId, SlideDocument>,
) -> RevisionId {
    let mut input = title.to_owned();
    for id in order {
        input.push_str(id.as_str());
        if let Some(slide) = slides.get(id) {
            input.push_str(slide.revision.as_str());
        }
    }
    revision_for(&input)
}

fn slide_revision_path(path: &str) -> Option<&str> {
    path.strip_prefix("/slides/")?.strip_suffix("/revision")
}

fn slide_markdown_path(path: &str) -> Option<&str> {
    path.strip_prefix("/slides/")?.strip_suffix("/markdown")
}

fn added_slide_path(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/slides/")?;
    (!id.contains('/')).then_some(id)
}

fn removed_slide_path(path: &str) -> Option<&str> {
    added_slide_path(path)
}

fn is_order_path(path: &str, allow_append: bool) -> bool {
    path.strip_prefix("/order/")
        .is_some_and(|index| (allow_append && index == "-") || index.parse::<usize>().is_ok())
}

fn validate_slide_id(id: &str) -> Result<(), String> {
    validate_node_id(id, "slide")
}

fn split_slide_fragment(markdown: &str) -> Vec<String> {
    let fences = markdown_fences(markdown);
    let mut fragments = Vec::with_capacity(fences.len() + 1);
    let mut start = 0;
    for separator in fences {
        fragments.push(markdown[start..separator.start].trim().to_owned());
        start = separator.end;
    }
    fragments.push(markdown[start..].trim().to_owned());
    fragments
}

fn frontmatter_contains_key(frontmatter: &str, key: &str) -> bool {
    let prefix = format!("{key}:");
    frontmatter
        .lines()
        .any(|line| line.trim_start().starts_with(&prefix))
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/deck.rs"]
mod tests;
