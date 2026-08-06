use async_trait::async_trait;

use crate::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        ImageGenerationProvider, ImageGenerationRequest, ImageGenerationResponse,
        SpeechGenerationProvider, SpeechGenerationRequest, SpeechGenerationResponse,
        VideoGenerationProvider, VideoGenerationRequest, VideoGenerationResponse,
    },
    sources::SourceDocument,
};

pub mod documents;
pub mod narration;
pub mod pages;
pub(crate) mod project_assets;
pub mod slides;
pub mod videos;

/// Truncates text destined for a prompt, saying so when it truncates.
///
/// The marker is the point. Two of the copies this replaces cut silently, and
/// both fed repair prompts — the invalid response handed back for correction, and
/// the draft passed into validation repair. A model asked to repair a document
/// whose tail was removed without a word is liable to "fix" a structure that only
/// looks unterminated: re-closing sections that were fine, or regenerating a tail
/// that already existed.
///
/// Naming sfumato in the marker matters too: it tells the model the cut is the
/// tool's doing and not something to repair.
pub(crate) fn excerpt(content: &str, max_chars: usize) -> String {
    let mut excerpt = content.chars().take(max_chars).collect::<String>();
    if content.chars().count() > max_chars {
        excerpt.push_str("\n[...truncated by sfumato...]");
    }
    excerpt
}

/// Largest source index rendered into a prompt, in characters.
///
/// Generous because the index is paths, not prose: several hundred files fit
/// well inside it. It exists so a vault pointed at wholesale cannot silently
/// reintroduce the very problem this index solves.
const MAX_SOURCE_INDEX_CHARS: usize = 8_000;

/// Renders the supplied sources as a directory tree instead of their content.
///
/// This is the whole grounding strategy, so it is worth stating why it is a
/// listing. Inlining every source spends the context window before the model has
/// read a word: the budget that used to do it truncated each file to fit, which
/// meant the model reasoned over amputated notes, and — because the agent loop
/// resends the conversation on every tool round — paid for that dump again on
/// each round. Both costs grow with the size of the vault, and neither buys
/// relevance: most files supplied to a request have nothing to do with it.
///
/// A tree inverts that. The model sees everything that exists, then reads the
/// few files it judges relevant, in full, through `sfumato_read_file`. Cost now
/// tracks what the request actually needs rather than what was pointed at.
///
/// Sizes are shown because the model is choosing what to spend context on, and a
/// title is shown when it says something the filename does not.
pub(crate) fn build_source_index(documents: &[SourceDocument]) -> String {
    if documents.is_empty() {
        return "No explicit source files were supplied.".to_string();
    }

    let total_chars: usize = documents
        .iter()
        .map(|document| document.content.chars().count())
        .sum();
    let mut rendered = format!(
        "{} source file(s), {} total. This is an index, not the content: \
         call `sfumato_read_file` with a path below to read one in full.\n",
        documents.len(),
        human_size(total_chars)
    );

    // Grouped by directory so the listing reads as the tree it describes, and so
    // a model scanning it can dismiss a whole subject folder at once.
    let mut current_directory = None;
    let mut listed = 0usize;
    for document in documents {
        let directory = document.path.parent();
        if current_directory != Some(directory) {
            let heading = directory
                .map(|path| format!("\n{}/\n", path.display()))
                .unwrap_or_else(|| "\n./\n".to_string());
            if rendered.chars().count() + heading.chars().count() > MAX_SOURCE_INDEX_CHARS {
                break;
            }
            rendered.push_str(&heading);
            current_directory = Some(directory);
        }
        let name = document
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| document.path.display().to_string());
        let title = source_title(&document.content)
            .filter(|title| !title_repeats_filename(title, &name))
            .map(|title| format!(" — {title}"))
            .unwrap_or_default();
        let line = format!(
            "  {name}  [{}]{title}\n",
            human_size(document.content.chars().count())
        );
        if rendered.chars().count() + line.chars().count() > MAX_SOURCE_INDEX_CHARS {
            break;
        }
        rendered.push_str(&line);
        listed += 1;
    }

    // Named rather than silent: a model told nothing would treat a clipped index
    // as the whole corpus and never ask for what it cannot see.
    if listed < documents.len() {
        rendered.push_str(&format!(
            "\n[{} further file(s) omitted from this index because it grew too large. \
             Call `sfumato_list_directory` on a directory above to see the rest.]\n",
            documents.len() - listed
        ));
    }
    rendered
}

/// A source's own title, when it declares one.
///
/// Frontmatter first, then a top-level heading, and nothing otherwise. A `##`
/// would almost always match — it is the first section of the note — but a
/// section name is not a description: "El problema" tells a model choosing what
/// to read strictly less than the filename beside it already did.
fn source_title(content: &str) -> Option<String> {
    let frontmatter_title = content
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---").map(|(front, _)| front))
        .and_then(|front| {
            front.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("title:")
                    .map(|title| title.trim().trim_matches(['"', '\'']).to_string())
            })
        })
        .filter(|title| !title.is_empty());
    frontmatter_title
        .or_else(|| {
            content
                .lines()
                .take(40)
                .find_map(|line| line.trim().strip_prefix("# ").map(str::to_owned))
        })
        .map(|title| excerpt(title.trim(), 80))
}

/// Whether a heading only repeats the filename, and so is not worth the tokens.
fn title_repeats_filename(title: &str, filename: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    normalize(title) == normalize(stem)
}

/// Formats a character count for a reader choosing what to spend context on.
fn human_size(chars: usize) -> String {
    if chars < 1_000 {
        format!("{chars} chars")
    } else {
        format!("{:.1}k chars", chars as f64 / 1_000.0)
    }
}

pub(crate) struct DryRunImageProvider;

#[async_trait]
impl ImageGenerationProvider for DryRunImageProvider {
    async fn generate_image(
        &self,
        _request: ImageGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        Err(SfumatoError::provider(
            ErrorClass::Unavailable,
            "Dry-run image provider cannot execute",
        ))
    }
}

pub(crate) struct DryRunVideoProvider;

#[async_trait]
impl VideoGenerationProvider for DryRunVideoProvider {
    async fn generate_video(
        &self,
        _request: VideoGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<VideoGenerationResponse> {
        Err(SfumatoError::provider(
            ErrorClass::Unavailable,
            "Dry-run video provider cannot execute",
        ))
    }
}

pub(crate) struct DryRunSpeechProvider;

#[async_trait]
impl SpeechGenerationProvider for DryRunSpeechProvider {
    async fn generate_speech(
        &self,
        _request: SpeechGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<SpeechGenerationResponse> {
        Err(SfumatoError::provider(
            ErrorClass::Unavailable,
            "Dry-run speech provider cannot execute",
        ))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/resources.rs"]
mod tests;
