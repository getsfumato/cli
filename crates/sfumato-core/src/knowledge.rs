//! Knowledge-source ports: the brain a project may be grounded in.
//!
//! Sfumato grounds a resource in one of two ways. The default reads local
//! files, and the model reaches them with the filesystem tools. The other
//! queries a Vitruvio brain — a store that returns evidence with provenance and
//! never prose — and the model reaches it with a single search tool.
//!
//! The distinction lives here rather than in each workflow because it is one
//! decision with one question behind it: where may this resource's claims come
//! from? Everything downstream — drafting, validation, rendering, publishing —
//! is deliberately identical either way.

use std::{collections::BTreeMap, fmt, path::PathBuf, str::FromStr};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    errors::{OperationStage, SfumatoError, SfumatoResult as Result},
    operation::OperationContext,
    sfumato_bail as bail,
};

/// Everything one brain invocation needs to reach its brain.
///
/// Carried per call rather than held by the client so a single client instance
/// can serve every project: which brain to read is a property of the request,
/// not of the transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrainBinding {
    /// Brain name from the project's `vitruvio.toml`, or a path to one.
    pub brain: String,
    /// Optional explicit Vitruvio configuration file.
    pub config_file: Option<PathBuf>,
    /// Optional explicit executable, for a brain tool not on `PATH`.
    pub executable: Option<PathBuf>,
    /// Optional actor identity recorded against each query.
    pub actor: Option<String>,
    /// Wall-clock bound for one invocation.
    pub timeout_seconds: u64,
}

/// One of the brain's five typed memory modules.
///
/// The types are the point: without them "what happened in the class of May
/// 14" and "the definition of a Fourier series" compete in a single ranking,
/// which is the failure typed memories exist to prevent.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Evidence as it was registered.
    Canonical,
    /// What happened, and when.
    Episodic,
    /// Facts and claims.
    Semantic,
    /// Goals and the steps that reach them.
    Procedural,
    /// Who derived what from what.
    Provenance,
}

impl MemoryType {
    /// Every module, in the order the brain documents them.
    pub const ALL: [Self; 5] = [
        Self::Canonical,
        Self::Episodic,
        Self::Semantic,
        Self::Procedural,
        Self::Provenance,
    ];

    /// Stable identifier used on the wire and in configuration.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Episodic => "episodic",
            Self::Semantic => "semantic",
            Self::Procedural => "procedural",
            Self::Provenance => "provenance",
        }
    }
}

impl fmt::Display for MemoryType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MemoryType {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|memory_type| memory_type.as_str() == value.trim().to_ascii_lowercase())
            .ok_or_else(|| {
                // Naming the valid values is the whole message: this error is
                // read most often by a model that chose a plausible-sounding
                // module, and a refusal it cannot act on costs a whole round.
                SfumatoError::validation(format!(
                    "Unknown memory type '{value}'. Use one of: {}.",
                    MemoryType::ALL.map(MemoryType::as_str).join(", ")
                ))
            })
    }
}

/// A hint about how the brain should retrieve, never a choice of index.
///
/// The brain's planner decides which indices to consult; a mode restricts the
/// plans it will consider. Sfumato passes the hint through and interprets
/// nothing.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    /// Let the planner decide.
    Auto,
    /// Identity lookup.
    Exact,
    /// Term matching.
    Lexical,
    /// Embedding similarity.
    Semantic,
    /// Graph neighbourhood.
    Associative,
}

impl RetrievalMode {
    /// Every mode, in the order the brain documents them.
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Exact,
        Self::Lexical,
        Self::Semantic,
        Self::Associative,
    ];

    /// Stable identifier used on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Exact => "exact",
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Associative => "associative",
        }
    }
}

impl fmt::Display for RetrievalMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RetrievalMode {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == value.trim().to_ascii_lowercase())
            .ok_or_else(|| {
                SfumatoError::validation(format!(
                    "Unknown retrieval mode '{value}'. Use one of: {}.",
                    RetrievalMode::ALL.map(RetrievalMode::as_str).join(", ")
                ))
            })
    }
}

/// One question put to the brain.
#[derive(Clone, Debug)]
pub struct BrainSearchRequest {
    /// Which brain to ask.
    pub binding: BrainBinding,
    /// What to look for, in natural language or terms.
    pub question: String,
    /// Modules to restrict to; empty means the brain decides.
    pub memory_types: Vec<MemoryType>,
    /// Restrict to one subject.
    pub subject: Option<String>,
    /// Require these tags.
    pub tags: Vec<String>,
    /// RFC3339 lower bound on when a block occurred.
    pub since: Option<String>,
    /// RFC3339 upper bound.
    pub until: Option<String>,
    /// Include blocks a newer one has replaced.
    pub include_superseded: bool,
    /// Retrieval hint.
    pub mode: Option<RetrievalMode>,
    /// How many matches to return.
    pub limit: usize,
    /// How far to expand along graph edges from the strongest hits.
    pub expand_depth: u8,
}

/// A request for the brain's inventory.
#[derive(Clone, Debug)]
pub struct BrainCardRequest {
    /// Which brain to describe.
    pub binding: BrainBinding,
}

/// A canonical block one match cites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceSource {
    /// Identity of the cited block.
    pub block_id: String,
    /// Where inside that block the claim sits, when the brain knows.
    pub locator: Option<String>,
}

/// One block the brain returned.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceMatch {
    /// Content-addressed identity.
    pub block_id: String,
    /// Module the block lives in.
    pub memory_type: MemoryType,
    /// The block's payload, passed through untouched.
    pub content: serde_json::Value,
    /// Agreement between retrieval strategies, as the brain printed it.
    ///
    /// Kept as a string on purpose. It is not a probability and not a
    /// confidence, and parsing it into a float is the first step towards
    /// presenting it as one.
    pub score: String,
    /// Canonical blocks this match cites.
    pub sources: Vec<EvidenceSource>,
    /// Whether the block verified against its module root.
    pub verified: bool,
    /// Whether the block's content is installed and unredacted.
    pub resolvable: bool,
    /// Identity of the block that replaced this one, when one has.
    pub superseded_by: Option<String>,
}

/// What the planner did, when the brain reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetrievalPlan {
    /// Stable signature of the chosen plan.
    pub signature: Option<String>,
    /// Classified intent of the question.
    pub intent: Option<String>,
    /// Indices the plan actually read.
    pub indices_consulted: Vec<String>,
    /// Ways the retrieval fell short of the ideal plan.
    pub degradations: Vec<String>,
}

/// Everything one search returned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvidenceBundle {
    /// Blocks that matched, strongest first.
    pub matches: Vec<EvidenceMatch>,
    /// Module roots the matches were verified against.
    pub verified_against: BTreeMap<MemoryType, String>,
    /// Whether candidates were dropped, so more evidence exists.
    pub truncated: bool,
    /// Whether every returned match verified.
    pub all_verified: bool,
    /// Planner detail, when the brain reported any.
    pub plan: Option<RetrievalPlan>,
    /// Warnings the brain emitted alongside the result.
    pub warnings: Vec<String>,
}

/// One memory module as the brain reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrainModule {
    /// Which module.
    pub memory_type: MemoryType,
    /// How many blocks it holds.
    pub block_count: u64,
    /// How many of those are installed and readable, when known.
    pub resolvable: Option<u64>,
    /// Merkle root the module verifies against.
    pub root: Option<String>,
    /// Index kinds built over it.
    pub indices: Vec<String>,
    /// Whether the indices are current.
    pub freshness: Option<String>,
}

/// A column the brain can filter on, and what it knows about its values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrainFacet {
    /// Column name, such as `subject` or `tag`.
    pub name: String,
    /// How many distinct values it holds.
    pub distinct: u64,
    /// The most frequent values, when the brain enumerates them.
    ///
    /// Empty against a brain whose statistics report only a count. The card
    /// then says how many values exist rather than pretending to list them.
    pub top: Vec<(String, u64)>,
}

/// The brain's inventory, shown to the model in place of a file index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrainCard {
    /// Brain name or reference.
    pub brain: String,
    /// Snapshot digest of the composition currently installed.
    pub snapshot: Option<String>,
    /// Modules present, in protocol order.
    pub modules: Vec<BrainModule>,
    /// Filterable columns and their value counts.
    pub facets: Vec<BrainFacet>,
    /// Indices that travel inside the brain artifact rather than being rebuilt.
    pub travelling_indices: Vec<String>,
    /// Anything that degraded while the card was assembled.
    pub warnings: Vec<String>,
}

/// One question put to the brain during a run, and what came back.
///
/// Recorded because the compact retry builds a fresh request with no tools and
/// no transcript: without a log of what was already retrieved, a brain-backed
/// project would redraft from nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct BrainEvidenceRecord {
    /// The question as the model phrased it.
    pub question: String,
    /// Filters it narrowed with, rendered for a reader.
    pub filters: Vec<String>,
    /// What the brain answered.
    pub bundle: EvidenceBundle,
}

/// Port for querying a knowledge brain.
///
/// Nothing here mentions a process, an exit code, or stdout: the shipped
/// adapter drives the `vitruvio` CLI, and an HTTP one can replace it without
/// core noticing.
#[async_trait]
pub trait BrainClient: Send + Sync {
    /// Describes what the brain holds, once per run.
    async fn card(
        &self,
        request: BrainCardRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<BrainCard>;

    /// Puts one question to the brain and returns its evidence.
    async fn search(
        &self,
        request: BrainSearchRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<EvidenceBundle>;
}

/// Decides where one operation's claims may come from.
///
/// Called by every resource workflow at the point it builds its tool set, which
/// is the first place the effective configuration and the requested sources are
/// both in hand. Making the choice here rather than when the application is
/// wired is what lets the backend be a per-project setting: the ports are built
/// once for the process, and the grounding is data that travels with a request.
pub fn resolve_grounding(
    config: &crate::config::EffectiveConfig,
    requested_sources: &[PathBuf],
    client: &std::sync::Arc<dyn BrainClient>,
) -> Result<crate::tools::Grounding> {
    if !config.knowledge.uses_brain() {
        return Ok(crate::tools::Grounding::Filesystem {
            sources: requested_sources.to_vec(),
        });
    }
    let binding = config.brain_binding()?;
    refuse_sources_under_brain(&binding.brain, requested_sources)?;
    Ok(crate::tools::Grounding::Brain(
        crate::tools::BrainToolConfig {
            client: client.clone(),
            binding,
            defaults: crate::tools::BrainQueryDefaults {
                memory_types: config.knowledge.memory_types.clone(),
                include_superseded: config.knowledge.include_superseded,
                default_limit: config.knowledge.default_limit,
                max_limit: config.knowledge.max_limit,
            },
        },
    ))
}

/// Refuses source paths against a project that is grounded in a brain.
///
/// Refusing beats ignoring. A silently dropped `--sources` leaves someone
/// believing a file grounded the resource when nothing did, and the resource
/// looks exactly the same either way.
pub fn refuse_sources_under_brain(brain: &str, sources: &[PathBuf]) -> Result<()> {
    if sources.is_empty() {
        return Ok(());
    }
    bail!(
        "This project is grounded in the brain '{brain}', so source paths are not read. \
         Remove them, or set knowledge.backend = \"filesystem\" in .sfumato/project.toml."
    )
}

#[cfg(test)]
#[path = "../tests/unit/knowledge.rs"]
mod tests;
