use async_trait::async_trait;

use std::collections::BTreeSet;

use crate::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    knowledge::{BrainCard, BrainEvidenceRecord, BrainFacet},
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

/// Largest brain card rendered into a prompt, in characters.
///
/// Smaller than the source index because a card is a fixed handful of rows —
/// five modules and a facet line — rather than a listing that grows with the
/// corpus. A brain that overruns this is reporting something unexpected, and
/// clipping it is safer than letting it crowd out the instruction.
const MAX_BRAIN_CARD_CHARS: usize = 4_000;

/// Renders what the brain holds, in place of a listing of files.
///
/// The brain-backed counterpart to [`build_source_index`], and it answers the
/// same question: what exists, so the model can decide what to ask for. It
/// cannot be a listing — a brain has no paths — so it is an inventory. Block
/// counts tell the model whether a module is worth interrogating at all, and
/// facets tell it which filters will actually narrow anything.
///
/// Roots are shown because they are what makes a claim checkable afterwards,
/// and a card that hid them would present the brain as an oracle rather than as
/// evidence with a verifiable provenance.
pub(crate) fn build_brain_card(card: &BrainCard) -> String {
    let mut rendered = format!("Brain: {}", card.brain);
    if let Some(snapshot) = &card.snapshot {
        rendered.push_str(&format!(" ({snapshot})"));
    }
    rendered.push('\n');

    if card.modules.is_empty() {
        rendered.push_str(
            "\nThis brain has no installed modules. It can be queried, but it holds nothing \
             to answer with.\n",
        );
        return rendered;
    }

    rendered.push_str("\nmodule       blocks  indices\n");
    for module in &card.modules {
        let indices = if module.indices.is_empty() {
            "(none)".to_string()
        } else {
            module.indices.join(", ")
        };
        rendered.push_str(&format!(
            "{:<12} {:>6}  {indices}\n",
            module.memory_type.as_str(),
            module.block_count
        ));
    }

    // A column with one distinct value cannot narrow anything, and the brain
    // reports several of those as a matter of course — `memory_type`, and the
    // internal flags of whatever happens to be stored. Listing them fills the
    // card with filters that all return everything.
    let facets = card
        .facets
        .iter()
        .filter(|facet| facet.distinct > 1)
        .map(describe_facet)
        .collect::<Vec<_>>();
    if !facets.is_empty() {
        rendered.push_str(&format!("\nFilterable by: {}\n", facets.join(", ")));
    }

    // Named rather than left out: a module whose index was never built still
    // answers, but it answers worse, and a model told nothing would read a thin
    // result as an empty brain.
    let unindexed = card
        .modules
        .iter()
        .filter(|module| module.indices.is_empty() && module.block_count > 0)
        .map(|module| module.memory_type.as_str())
        .collect::<Vec<_>>();
    if !unindexed.is_empty() {
        rendered.push_str(&format!(
            "\nNo index is built over {}, so searching there recalls less than it could.\n",
            unindexed.join(", ")
        ));
    }

    for warning in &card.warnings {
        rendered.push_str(&format!("\n[{warning}]\n"));
    }

    excerpt(&rendered, MAX_BRAIN_CARD_CHARS)
}

/// Describes one filterable column, enumerating its values only when known.
///
/// Vitruvio reports a column as a count of distinct values today, so the honest
/// rendering says how many there are rather than inventing examples. When it
/// starts reporting the frequent values, they appear here without further work.
fn describe_facet(facet: &BrainFacet) -> String {
    if facet.top.is_empty() {
        return format!("{} ({} values)", facet.name, facet.distinct);
    }
    let values = facet
        .top
        .iter()
        .map(|(value, _)| value.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if facet.top.len() as u64 >= facet.distinct {
        format!("{} ({values})", facet.name)
    } else {
        format!(
            "{} ({} values, most common: {values})",
            facet.name, facet.distinct
        )
    }
}

/// Renders evidence already retrieved, for a retry that has no tools.
///
/// The compact retry drops the tools and the transcript and asks again from a
/// clean prompt. Under a brain there is no file to inline in their place, so
/// what the brain already answered is the only material there is.
///
/// Unverified and superseded matches are dropped first rather than budgeted
/// alongside the rest: the prompt forbids writing from them either way, so
/// spending characters on them would push out material the model may actually
/// use.
pub(crate) fn build_compact_evidence_bundle(
    records: &[BrainEvidenceRecord],
    max_chars: usize,
) -> String {
    let mut seen = BTreeSet::new();
    let mut usable = Vec::new();
    for record in records {
        for matched in &record.bundle.matches {
            if !matched.verified || !matched.resolvable || matched.superseded_by.is_some() {
                continue;
            }
            if seen.insert(matched.block_id.clone()) {
                usable.push(matched);
            }
        }
    }

    if usable.is_empty() {
        return "No usable evidence was retrieved from the brain before this retry. \
                Write only what the instruction and the brain card below already support, \
                and leave out anything you cannot ground."
            .to_string();
    }

    let questions = records
        .iter()
        .map(|record| record.question.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    let mut rendered = format!(
        "{} verified block(s) retrieved from the brain, in answer to: {questions}.\n",
        usable.len()
    );

    // Split evenly rather than first-come: a single long block would otherwise
    // consume the budget and silently hide every block after it.
    let budget = max_chars.saturating_sub(rendered.chars().count());
    let per_block = (budget / usable.len()).max(200);
    for matched in usable {
        let content = serde_json::to_string_pretty(&matched.content)
            .unwrap_or_else(|_| matched.content.to_string());
        rendered.push_str(&format!(
            "\n[{} · {}]\n{}\n",
            matched.memory_type.as_str(),
            matched.block_id,
            excerpt(&content, per_block)
        ));
    }
    excerpt(&rendered, max_chars)
}

/// What a brain-grounded workflow keeps so it can rebuild a tool-less prompt.
///
/// The tool set is consumed by the draft request, but the compact retry is
/// built afterwards and needs the brain again. Cloning the two cheap halves of
/// the grounding is simpler than threading the whole tool config through.
#[derive(Clone)]
pub(crate) struct BrainRetryContext {
    pub(crate) client: std::sync::Arc<dyn crate::knowledge::BrainClient>,
    pub(crate) binding: crate::knowledge::BrainBinding,
    pub(crate) defaults: crate::tools::BrainQueryDefaults,
}

impl BrainRetryContext {
    /// Clones what a retry needs out of a grounding, when it is a brain.
    pub(crate) fn from_grounding(grounding: &crate::tools::Grounding) -> Option<Self> {
        match grounding {
            crate::tools::Grounding::Filesystem { .. } => None,
            crate::tools::Grounding::Brain(config) => Some(Self {
                client: config.client.clone(),
                binding: config.binding.clone(),
                defaults: config.defaults.clone(),
            }),
        }
    }

    /// Renders the material a tool-less retry writes from.
    ///
    /// Normally that is whatever the model already retrieved. When it retrieved
    /// nothing — the context limit was reached on the first round, before any
    /// tool call — one deterministic query stands in, because a retry with an
    /// empty prompt would draft from nothing at all.
    ///
    /// Never fails: a retry that cannot be grounded still runs, and says so, so
    /// the model writes a short honest resource instead of an invented one.
    pub(crate) async fn compact_bundle(
        &self,
        tool_set: &crate::tools::ToolSet,
        instruction: &str,
        max_chars: usize,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> String {
        let mut records = tool_set.retrieved_evidence().unwrap_or_default();
        if records.is_empty() {
            let request = crate::knowledge::BrainSearchRequest {
                binding: self.binding.clone(),
                question: instruction.to_string(),
                memory_types: self.defaults.memory_types.clone(),
                subject: None,
                tags: Vec::new(),
                since: None,
                until: None,
                include_superseded: false,
                mode: None,
                limit: self.defaults.default_limit,
                expand_depth: 0,
            };
            match self.client.search(request, operation, stage).await {
                Ok(bundle) => records.push(crate::knowledge::BrainEvidenceRecord {
                    question: instruction.to_string(),
                    filters: Vec::new(),
                    bundle,
                }),
                Err(error) => {
                    return format!(
                        "No evidence could be retrieved from the brain for this retry ({error}). \
                         Write only what the instruction and the brain card support, and leave \
                         out anything you cannot ground."
                    );
                }
            }
        }
        build_compact_evidence_bundle(&records, max_chars)
    }
}

/// Resolves the material a tool-less retry writes from, at most once.
///
/// Memoized because the same bundle feeds the draft retry and, later, the
/// review retry, and under a brain the second call would otherwise re-query.
/// `fallback` is the filesystem compaction each workflow already builds; it is
/// cheap and pure, so it is computed eagerly and simply goes unused when the
/// project is grounded in a brain.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn compact_source_material(
    cache: &mut Option<String>,
    fallback: &str,
    brain: Option<&BrainRetryContext>,
    tool_set: &crate::tools::ToolSet,
    instruction: &str,
    max_chars: usize,
    operation: &OperationContext,
    stage: OperationStage,
) -> String {
    if let Some(cached) = cache {
        return cached.clone();
    }
    let rendered = match brain {
        None => fallback.to_string(),
        Some(retry) => {
            retry
                .compact_bundle(tool_set, instruction, max_chars, operation, stage)
                .await
        }
    };
    *cache = Some(rendered.clone());
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
