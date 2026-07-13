use std::{
    collections::{BTreeMap, BTreeSet, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use anyhow::{Context, Result, bail};
use json_patch::Patch;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PatchReport, ReviewFormat, ReviewSnapshot, ReviewableDocument, ValidationReport};

pub const SLIDE_DECK_SCHEMA_VERSION: u32 = 1;
const MAX_PATCH_OPERATIONS: usize = 32;

#[derive(Clone, Debug)]
pub struct SlideDeckDocument {
    schema_version: u32,
    revision: String,
    title: String,
    order: Vec<SlideId>,
    slides: BTreeMap<SlideId, SlideDocument>,
    frontmatter: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SlideId(pub String);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SlideDocument {
    pub revision: String,
    pub kind: SlideKind,
    pub heading: Option<String>,
    pub markdown: String,
    pub elements: Vec<SlideElement>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlideKind {
    Title,
    Section,
    #[default]
    Content,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlideElement {
    Paragraph,
    List { items: usize },
    Code { language: Option<String> },
    Mermaid,
    Math,
    Table { rows: usize, columns: usize },
    Image { source: String },
    PresenterNotes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PatchableSlideDeck {
    schema_version: u32,
    revision: String,
    title: String,
    order: Vec<SlideId>,
    slides: BTreeMap<SlideId, SlideDocument>,
}

impl SlideDeckDocument {
    pub fn from_marp(markdown: &str, title: &str) -> Result<Self> {
        let fences = markdown_fences(markdown);
        if fences.len() < 3
            || fences[0].start != 0
            || !frontmatter_contains_key(&markdown[fences[0].end..fences[1].start], "marp")
        {
            bail!("Cannot build a slide deck document from invalid Marp Markdown");
        }

        let frontmatter = markdown[..fences[1].end].trim().to_string();
        let mut fragments = Vec::new();
        let mut start = fences[1].end;
        for separator in fences.iter().skip(2) {
            fragments.push(markdown[start..separator.start].trim().to_string());
            start = separator.end;
        }
        fragments.push(markdown[start..].trim().to_string());

        let mut order = Vec::new();
        let mut slides = BTreeMap::new();
        for (index, fragment) in fragments.into_iter().enumerate() {
            let id = SlideId(format!("slide-{}", index + 1));
            order.push(id.clone());
            slides.insert(id, slide_from_markdown(fragment, index == 0));
        }

        let mut document = Self {
            schema_version: SLIDE_DECK_SCHEMA_VERSION,
            revision: String::new(),
            title: title.to_string(),
            order,
            slides,
            frontmatter,
        };
        document.refresh_metadata();
        document.validate()?;
        Ok(document)
    }

    pub fn slide_count(&self) -> usize {
        self.order.len()
    }

    fn patchable(&self) -> PatchableSlideDeck {
        PatchableSlideDeck {
            schema_version: self.schema_version,
            revision: self.revision.clone(),
            title: self.title.clone(),
            order: self.order.clone(),
            slides: self.slides.clone(),
        }
    }

    fn validate_patch_intent(&self, patch: &Patch) -> Result<()> {
        let operations = serde_json::to_value(patch)?;
        let operations = operations
            .as_array()
            .context("JSON Patch must be an array")?;
        if operations.len() > MAX_PATCH_OPERATIONS {
            bail!("Reviewer patch exceeds the maximum of {MAX_PATCH_OPERATIONS} operations");
        }
        if operations.is_empty() {
            return Ok(());
        }

        let title_slide = self
            .order
            .first()
            .context("Slide deck does not contain a title slide")?;
        let mut tested_deck = false;
        let mut tested_slides = HashSet::new();

        for operation in operations {
            let kind = operation
                .get("op")
                .and_then(Value::as_str)
                .context("JSON Patch operation is missing `op`")?;
            let path = operation
                .get("path")
                .and_then(Value::as_str)
                .context("JSON Patch operation is missing `path`")?;

            if kind == "test" {
                if path == "/revision" {
                    tested_deck = true;
                } else if let Some(id) = slide_revision_path(path) {
                    tested_slides.insert(id.to_string());
                } else {
                    bail!("Reviewer may only test the deck or slide revision, not `{path}`");
                }
                continue;
            }

            if !tested_deck {
                bail!("Reviewer patch must test `/revision` before making changes");
            }

            match kind {
                "replace" => {
                    let id = slide_markdown_path(path).with_context(|| {
                        format!("Reviewer may only replace slide Markdown, not `{path}`")
                    })?;
                    self.require_existing_mutable_slide(id, title_slide, &tested_slides)?;
                }
                "add" => {
                    if let Some(id) = added_slide_path(path) {
                        if self.slides.contains_key(&SlideId(id.to_string())) {
                            bail!("Reviewer cannot add existing slide `{id}`");
                        }
                        validate_slide_id(id)?;
                    } else if !is_order_path(path, true) {
                        bail!("Reviewer cannot add content at `{path}`");
                    }
                }
                "remove" => {
                    if let Some(id) = removed_slide_path(path) {
                        self.require_existing_mutable_slide(id, title_slide, &tested_slides)?;
                    } else if !is_order_path(path, false) {
                        bail!("Reviewer cannot remove content at `{path}`");
                    }
                }
                "move" => {
                    let from = operation
                        .get("from")
                        .and_then(Value::as_str)
                        .context("JSON Patch move operation is missing `from`")?;
                    if !is_order_path(path, true) || !is_order_path(from, false) {
                        bail!("Reviewer may only move entries inside `/order`");
                    }
                }
                _ => bail!("Reviewer patch operation `{kind}` is not allowed"),
            }
        }
        Ok(())
    }

    fn require_existing_mutable_slide(
        &self,
        id: &str,
        title_slide: &SlideId,
        tested_slides: &HashSet<String>,
    ) -> Result<()> {
        let id = SlideId(id.to_string());
        if &id == title_slide {
            bail!("Reviewer cannot modify or remove the title slide");
        }
        if !self.slides.contains_key(&id) {
            bail!("Reviewer referenced unknown slide `{}`", id.0);
        }
        if !tested_slides.contains(&id.0) {
            bail!(
                "Reviewer patch must test `/slides/{}/revision` before changing that slide",
                id.0
            );
        }
        Ok(())
    }

    fn refresh_metadata(&mut self) {
        for (index, id) in self.order.iter().enumerate() {
            if let Some(slide) = self.slides.get_mut(id) {
                let markdown = slide.markdown.trim().to_string();
                *slide = slide_from_markdown(markdown, index == 0);
            }
        }
        self.revision = deck_revision(&self.title, &self.order, &self.slides);
    }
}

impl ReviewableDocument for SlideDeckDocument {
    fn snapshot(&self) -> Result<ReviewSnapshot> {
        Ok(ReviewSnapshot {
            schema_version: SLIDE_DECK_SCHEMA_VERSION,
            format: ReviewFormat::SlideDeck,
            revision: self.revision.clone(),
            document: serde_json::to_value(self.patchable())?,
            constraints: vec![
                "Return only RFC 6902 JSON Patch operations.".to_string(),
                "Test /revision before any mutation.".to_string(),
                "Test /slides/<id>/revision before replacing or removing that slide.".to_string(),
                "Do not modify the title slide, title, frontmatter, IDs, revisions, or derived metadata."
                    .to_string(),
                "Prefer replacing /slides/<id>/markdown; use add/remove/order operations only when structure must change."
                    .to_string(),
            ],
        })
    }

    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport> {
        self.validate_patch_intent(patch)?;
        let original_count = self.slide_count();
        let original_slides = self.slides.clone();
        let mut candidate_value = serde_json::to_value(self.patchable())?;
        json_patch::patch(&mut candidate_value, patch).context("Could not apply reviewer patch")?;
        refresh_slide_metadata_json(&mut candidate_value)?;

        let patched: PatchableSlideDeck = serde_json::from_value(candidate_value)
            .context("Reviewer patch produced an invalid slide deck structure")?;
        let mut candidate = Self {
            schema_version: patched.schema_version,
            revision: patched.revision,
            title: patched.title,
            order: patched.order,
            slides: patched.slides,
            frontmatter: self.frontmatter.clone(),
        };
        candidate.refresh_metadata();
        candidate.validate()?;

        if candidate.slide_count() * 10 < original_count * 7 {
            bail!(
                "Reviewer patch would reduce the deck from {original_count} to {} slides",
                candidate.slide_count()
            );
        }

        let changed_nodes = original_slides
            .keys()
            .chain(candidate.slides.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|id| original_slides.get(*id) != candidate.slides.get(*id))
            .map(|id| id.0.clone())
            .collect();
        let operations = patch.0.len();
        *self = candidate;
        Ok(PatchReport {
            operations,
            changed_nodes,
        })
    }

    fn validate(&self) -> Result<ValidationReport> {
        if self.schema_version != SLIDE_DECK_SCHEMA_VERSION {
            bail!(
                "Unsupported slide deck schema version {}",
                self.schema_version
            );
        }
        if self.title.trim().is_empty() {
            bail!("Slide deck title cannot be empty");
        }
        if self.order.len() < 2 {
            bail!("Slide deck must contain at least two slides");
        }
        let unique = self.order.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.order.len() {
            bail!("Slide deck order contains duplicate slide IDs");
        }
        if self.slides.len() != self.order.len()
            || self.order.iter().any(|id| !self.slides.contains_key(id))
        {
            bail!("Slide deck order and slide map do not contain the same slides");
        }
        let title_slide = self
            .slides
            .get(&self.order[0])
            .context("Slide deck title slide is missing")?;
        if title_slide.kind != SlideKind::Title
            || title_slide.heading.as_deref() != Some(self.title.as_str())
        {
            bail!("Slide deck title slide must preserve the deck title");
        }
        for (id, slide) in &self.slides {
            validate_slide_id(&id.0)?;
            if slide.markdown.trim().is_empty() {
                bail!("Slide `{}` cannot be empty", id.0);
            }
            validate_fenced_code_blocks(&slide.markdown, &id.0)?;
            if !markdown_fences(&slide.markdown).is_empty() {
                bail!(
                    "Slide `{}` contains a top-level `---`; add a separate slide instead",
                    id.0
                );
            }
        }
        Ok(ValidationReport::default())
    }

    fn render(&self) -> Result<String> {
        self.validate()?;
        let body = self
            .order
            .iter()
            .map(|id| self.slides[id].markdown.trim())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Ok(format!("{}\n\n{}", self.frontmatter.trim(), body))
    }
}

fn refresh_slide_metadata_json(value: &mut Value) -> Result<()> {
    let slides = value
        .get_mut("slides")
        .and_then(Value::as_object_mut)
        .context("Patched deck must contain a slide object")?;
    for slide in slides.values_mut() {
        let object = slide
            .as_object_mut()
            .context("Each patched slide must be a JSON object")?;
        let markdown = object
            .get("markdown")
            .and_then(Value::as_str)
            .context("Each patched slide must contain Markdown")?
            .trim()
            .to_string();
        let derived = slide_from_markdown(markdown, false);
        object.insert("revision".to_string(), Value::String(derived.revision));
        object.insert("kind".to_string(), serde_json::to_value(derived.kind)?);
        object.insert(
            "heading".to_string(),
            serde_json::to_value(derived.heading)?,
        );
        object.insert(
            "elements".to_string(),
            serde_json::to_value(derived.elements)?,
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
    if markdown.contains("```mermaid") {
        elements.push(SlideElement::Mermaid);
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
    if elements.is_empty() {
        elements.push(SlideElement::Paragraph);
    }
    elements
}

fn first_heading(markdown: &str) -> Option<String> {
    let mut code_fence: Option<(char, usize)> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
        } else if code_fence.is_none() {
            let heading = trimmed.trim_start_matches('#');
            if heading.len() != trimmed.len() && heading.starts_with(' ') {
                return Some(clean_heading(heading));
            }
        }
    }
    None
}

fn clean_heading(heading: &str) -> String {
    let mut heading = heading.trim().trim_end_matches('#').trim();
    loop {
        let mut changed = false;
        for delimiter in ["**", "__", "`", "*", "_"] {
            if let Some(inner) = heading
                .strip_prefix(delimiter)
                .and_then(|value| value.strip_suffix(delimiter))
            {
                heading = inner.trim();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    heading.to_string()
}

fn deck_revision(
    title: &str,
    order: &[SlideId],
    slides: &BTreeMap<SlideId, SlideDocument>,
) -> String {
    let mut input = title.to_string();
    for id in order {
        input.push_str(&id.0);
        if let Some(slide) = slides.get(id) {
            input.push_str(&slide.revision);
        }
    }
    revision_for(&input)
}

fn revision_for(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
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

fn validate_slide_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        bail!("Invalid slide ID `{id}`; use lowercase letters, numbers, and hyphens");
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Fence {
    start: usize,
    end: usize,
}

fn markdown_fences(markdown: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut cursor = 0;
    let mut code_fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = line_without_newline.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
        } else if code_fence.is_none() && line_without_newline.trim() == "---" {
            fences.push(Fence {
                start: cursor,
                end: cursor + line.len(),
            });
        }
        cursor += line.len();
    }
    fences
}

fn markdown_code_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = line.chars().take_while(|value| *value == marker).count();
    (length >= 3).then_some((marker, length))
}

fn validate_fenced_code_blocks(markdown: &str, slide_id: &str) -> Result<()> {
    let mut open: Option<(char, usize, String)> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let Some((marker, length)) = markdown_code_fence(trimmed) else {
            continue;
        };
        match &open {
            Some((open_marker, open_length, _))
                if marker == *open_marker && length >= *open_length =>
            {
                open = None;
            }
            None => {
                let language = trimmed[length..].trim().to_string();
                open = Some((marker, length, language));
            }
            _ => {}
        }
    }
    if let Some((_, _, language)) = open {
        let kind = if language.is_empty() {
            "code".to_string()
        } else {
            language
        };
        bail!("Slide `{slide_id}` has an unclosed `{kind}` fenced code block");
    }
    Ok(())
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
#[path = "../../tests/unit/review_decks.rs"]
mod tests;
