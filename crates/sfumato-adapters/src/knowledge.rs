//! Vitruvio brain access over the `vitruvio` command-line interface.
//!
//! Vitruvio exposes no network service: a brain is a local directory and the
//! CLI is the supported way in. Its `--json` contract is narrow enough to build
//! on — exactly one object on stdout, everything else on stderr, stable error
//! codes and exit codes — so this adapter parses that envelope and translates
//! it into the domain types in [`sfumato_core::knowledge`].
//!
//! Nothing about the process leaks upwards. When Vitruvio grows an HTTP
//! interface, a second adapter replaces this one and no workflow changes.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde_json::Value;
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    knowledge::{
        BrainCard, BrainCardRequest, BrainClient, BrainFacet, BrainModule, BrainSearchRequest,
        EvidenceBundle, EvidenceMatch, EvidenceSource, MemoryType, RetrievalPlan,
    },
    operation::OperationContext,
};
use tokio::process::Command;

use crate::runtime::run_command_within;

/// Executable assumed present on `PATH` when the project names none.
const DEFAULT_EXECUTABLE: &str = "vitruvio";

/// How much standard error is quoted back when a call fails.
///
/// Vitruvio writes notes and progress there, so the whole stream is noise
/// around the one line that explains a failure — which is the last one.
const STDERR_TAIL_CHARS: usize = 400;

/// Reads a Vitruvio brain by driving its command-line interface.
///
/// Stateless, and the brain to read arrives with each request. That is what
/// lets one instance serve every project: Vitruvio selects its brain per
/// invocation, so there is no session to key by.
#[derive(Clone, Copy, Debug, Default)]
pub struct VitruvioCliBrainClient;

#[async_trait]
impl BrainClient for VitruvioCliBrainClient {
    async fn card(
        &self,
        request: BrainCardRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<BrainCard> {
        let binding = request.binding;
        let info = invoke(&binding, &["brain", "info"], operation, stage).await?;
        let mut card = parse_card(&info).map_err(|error| brain_error(&error, stage))?;

        // Statistics are an enrichment, not a precondition: a brain whose
        // indices were never built still answers queries, and failing the whole
        // run over a missing derived file would be the tool refusing to work
        // because it could not describe itself.
        match invoke(&binding, &["index", "stats"], operation, stage).await {
            Ok(stats) => apply_statistics(&mut card, &stats),
            Err(error) if error.class == ErrorClass::Cancelled => return Err(error),
            Err(error) => card
                .warnings
                .push(format!("index statistics are unavailable: {error}")),
        }
        Ok(card)
    }

    async fn search(
        &self,
        request: BrainSearchRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<EvidenceBundle> {
        let mut arguments = vec![
            "query".to_string(),
            "search".to_string(),
            request.question.clone(),
        ];
        for memory_type in &request.memory_types {
            arguments.push("--memory-type".to_string());
            arguments.push(memory_type.as_str().to_string());
        }
        if let Some(subject) = &request.subject {
            arguments.push("--subject".to_string());
            arguments.push(subject.clone());
        }
        for tag in &request.tags {
            arguments.push("--tag".to_string());
            arguments.push(tag.clone());
        }
        if let Some(since) = &request.since {
            arguments.push("--since".to_string());
            arguments.push(since.clone());
        }
        if let Some(until) = &request.until {
            arguments.push("--until".to_string());
            arguments.push(until.clone());
        }
        if request.include_superseded {
            arguments.push("--include-superseded".to_string());
        }
        if let Some(mode) = request.mode {
            arguments.push("--mode".to_string());
            arguments.push(mode.as_str().to_string());
        }
        arguments.push("--limit".to_string());
        arguments.push(request.limit.to_string());
        if request.expand_depth > 0 {
            arguments.push("--expand-depth".to_string());
            arguments.push(request.expand_depth.to_string());
        }

        let borrowed = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        let payload = invoke(&request.binding, &borrowed, operation, stage).await?;
        parse_bundle(&payload).map_err(|error| brain_error(&error, stage))
    }
}

/// One envelope Vitruvio wrote to standard output.
struct Envelope {
    data: Value,
    warnings: Vec<String>,
}

/// Runs one Vitruvio subcommand and returns its envelope payload.
async fn invoke(
    binding: &sfumato_core::knowledge::BrainBinding,
    arguments: &[&str],
    operation: &OperationContext,
    stage: OperationStage,
) -> SfumatoResult<Envelope> {
    let executable = executable_path(binding);
    // `vitruvio` may be a bare name on PATH or a configured path; resolve handles
    // both, and hands the name back unchanged when it finds nothing.
    let mut command = Command::new(crate::executables::resolve(&executable.to_string_lossy()));
    // Project first, then brain: that is the order Vitruvio resolves them in, and
    // stating both is what makes one invocation independent of the directory it
    // ran from and of whatever `vitruvio brain use` last recorded on the machine.
    if let Some(project) = &binding.project {
        command.arg("--project").arg(project);
    }
    command.arg("--brain").arg(&binding.brain);
    if let Some(config) = &binding.config_file {
        command.arg("--config").arg(config);
    }
    if let Some(actor) = &binding.actor {
        command.arg("--actor").arg(actor);
    }
    // Always `agent`: a model is driving, and a brain that records who asked
    // should not be told a human did.
    command.args(["--actor-kind", "agent", "--json", "--no-color"]);
    command.args(arguments);

    let output = run_command_within(
        &mut command,
        operation,
        stage,
        Duration::from_secs(binding.timeout_seconds.max(1)),
    )
    .await
    .map_err(|error| spawn_error(error, &executable, stage))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = tail(&String::from_utf8_lossy(&output.stderr));
    let code = output.status.code().unwrap_or(1);

    let envelope: Value = serde_json::from_str(stdout.trim()).map_err(|error| {
        // A non-JSON stdout is usually a Vitruvio that died before it could
        // write its envelope, and then stderr holds the only explanation there
        // is. Dropping it would leave a parse error and no cause.
        SfumatoError::tool(
            ErrorClass::Permanent,
            format!(
                "The brain did not answer with one JSON object ({error}). \
                 It exited with code {code}.{}",
                detail_suffix(&stderr)
            ),
        )
        .at_stage(stage)
    })?;

    let ok = envelope
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(code == 0);
    if code != 0 || !ok {
        return Err(envelope_error(&envelope, code, &stderr, stage));
    }

    Ok(Envelope {
        data: envelope.get("data").cloned().unwrap_or(Value::Null),
        warnings: string_list(envelope.get("warnings")),
    })
}

/// Which binary to run for this project.
fn executable_path(binding: &sfumato_core::knowledge::BrainBinding) -> PathBuf {
    let Some(executable) = &binding.executable else {
        return PathBuf::from(DEFAULT_EXECUTABLE);
    };
    // `~` is not expanded by the shell here because nothing spawns a shell, and
    // a configuration file is exactly where someone writes a home-relative path.
    let Ok(relative) = executable.strip_prefix("~") else {
        return executable.clone();
    };
    dirs::home_dir().map_or_else(|| executable.clone(), |home| home.join(relative))
}

/// Explains a Vitruvio that could not be started at all.
fn spawn_error(error: anyhow::Error, executable: &Path, stage: OperationStage) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        let mut error = error.clone();
        if error.stage.is_none() {
            error.stage = Some(stage);
        }
        return error;
    }
    let missing = error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound);
    if missing {
        return SfumatoError::config(format!(
            "The brain tool '{}' was not found. Install Vitruvio, or set knowledge.executable \
             in .sfumato/project.toml to its path.",
            executable.display()
        ))
        .at_stage(stage);
    }
    SfumatoError::tool(ErrorClass::Unavailable, format!("{error:#}")).at_stage(stage)
}

/// Translates a Vitruvio exit code and error object into a Sfumato error.
///
/// The classification matters more than the wording. A permanent error is
/// handed back to the model as a failed tool result so it can rephrase; an
/// unavailable one is worth another attempt; a configuration one names the key
/// that fixes it, because no rephrasing will.
fn envelope_error(
    envelope: &Value,
    code: i32,
    stderr: &str,
    stage: OperationStage,
) -> SfumatoError {
    let error = envelope.get("error").filter(|value| !value.is_null());
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The brain rejected the request")
        .to_string();
    let hint = error
        .and_then(|error| error.get("hint"))
        .and_then(Value::as_str)
        .map(|hint| format!(" {hint}"))
        .unwrap_or_default();
    let name = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let named = if name.is_empty() {
        String::new()
    } else {
        format!(" [{name}]")
    };
    let detail = if error.is_some() {
        String::new()
    } else {
        detail_suffix(stderr)
    };
    let message = format!("{message}{named}{hint}{detail}");

    match code {
        // Vitruvio's own code, not the exit code: an unregistered project and an
        // unreadable one both exit 3, and only the first is fixed by a command
        // rather than by editing anything.
        3 if name == "PROJECT_NOT_KNOWN" => SfumatoError::config(format!(
            "{message} Run `vitruvio project register` in that project's directory, \
             or correct knowledge.project in .sfumato/project.toml."
        )),
        3 => SfumatoError::config(format!(
            "{message} Check knowledge.project, knowledge.brain and knowledge.config \
             in .sfumato/project.toml."
        )),
        4 => SfumatoError::not_found(message),
        2 | 6 | 7 => SfumatoError::tool(ErrorClass::Permanent, message),
        0 => SfumatoError::tool(
            ErrorClass::Permanent,
            format!("{message} (the brain reported success and an error at once)"),
        ),
        _ => SfumatoError::tool(ErrorClass::Unavailable, message),
    }
    .at_stage(stage)
}

/// Wraps a parse failure that happened after a successful call.
fn brain_error(error: &anyhow::Error, stage: OperationStage) -> SfumatoError {
    SfumatoError::tool(ErrorClass::Permanent, format!("{error:#}")).at_stage(stage)
}

fn detail_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(" It said: {stderr}")
    }
}

fn tail(value: &str) -> String {
    let value = value.trim();
    let length = value.chars().count();
    if length <= STDERR_TAIL_CHARS {
        return value.to_string();
    }
    value
        .chars()
        .skip(length - STDERR_TAIL_CHARS)
        .collect::<String>()
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map_or_else(|| item.to_string(), ToString::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Reads `brain info` into the inventory shown to the model.
fn parse_card(envelope: &Envelope) -> Result<BrainCard> {
    let data = &envelope.data;
    let modules = data
        .get("modules")
        .and_then(Value::as_array)
        .context("The brain's anatomy carried no modules")?
        .iter()
        .filter_map(|module| {
            let memory_type = module
                .get("memory_type")
                .and_then(Value::as_str)
                .and_then(|value| MemoryType::from_str(value).ok())?;
            Some(BrainModule {
                memory_type,
                block_count: module
                    .get("block_count")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                resolvable: None,
                root: module
                    .get("root")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                indices: string_list(module.get("indices")),
                freshness: None,
            })
        })
        .collect();

    Ok(BrainCard {
        brain: data
            .get("brain")
            .and_then(Value::as_str)
            .unwrap_or("(unnamed)")
            .to_string(),
        snapshot: None,
        modules,
        facets: Vec::new(),
        travelling_indices: string_list(data.get("travelling_indices")),
        warnings: envelope.warnings.clone(),
    })
}

/// Folds `index stats` into a card that already knows its modules.
fn apply_statistics(card: &mut BrainCard, envelope: &Envelope) {
    card.warnings.extend(envelope.warnings.iter().cloned());
    let Some(statistics) = envelope.data.get("statistics").and_then(Value::as_array) else {
        return;
    };
    let mut facets: BTreeMap<String, BrainFacet> = BTreeMap::new();
    for entry in statistics {
        let Some(memory_type) = entry
            .get("memory_type")
            .and_then(Value::as_str)
            .and_then(|value| MemoryType::from_str(value).ok())
        else {
            continue;
        };
        if let Some(module) = card
            .modules
            .iter_mut()
            .find(|module| module.memory_type == memory_type)
        {
            module.resolvable = entry.get("resolvable").and_then(Value::as_u64);
            module.freshness = entry
                .get("freshness")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            // Preferred over `brain info`'s list, which reports what is
            // registered on the session rather than what is built on disk. At
            // inspect capability that is always empty, so trusting it made the
            // card announce that a fully indexed brain had no indices at all.
            let built = string_list(entry.get("indices"));
            if !built.is_empty() {
                module.indices = built;
            }
        }
        let Some(columns) = entry.get("columns").and_then(Value::as_object) else {
            continue;
        };
        for (name, column) in columns {
            let facet = parse_facet(name, column);
            facets
                .entry(facet.name.clone())
                .and_modify(|existing| {
                    // Facets are per module but read as one vocabulary. Summing
                    // overstates nothing a reader acts on — the card says how
                    // much there is to filter by, not how it is partitioned.
                    existing.distinct += facet.distinct;
                    existing.top.extend(facet.top.iter().cloned());
                })
                .or_insert(facet);
        }
    }
    for facet in facets.values_mut() {
        facet
            .top
            .sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        facet.top.truncate(8);
    }
    card.facets = facets.into_values().collect();
}

/// Reads one statistics column, in either shape Vitruvio may report.
///
/// Today `summary()` reduces a column to a count of distinct values even though
/// the catalogue holds the most frequent ones. Accepting both an integer and an
/// object means the card starts naming real subjects the day that changes,
/// without a second release here.
fn parse_facet(name: &str, column: &Value) -> BrainFacet {
    if let Some(distinct) = column.as_u64() {
        return BrainFacet {
            name: name.to_string(),
            distinct,
            top: Vec::new(),
        };
    }
    let distinct = column
        .get("distinct")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let top = column
        .get("top")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    let value = pair.first()?.as_str()?.to_string();
                    let count = pair.get(1).and_then(Value::as_u64).unwrap_or_default();
                    Some((value, count))
                })
                .collect()
        })
        .unwrap_or_default();
    BrainFacet {
        name: name.to_string(),
        distinct,
        top,
    }
}

/// Reads `query search` into an evidence bundle.
fn parse_bundle(envelope: &Envelope) -> Result<EvidenceBundle> {
    let data = &envelope.data;
    let matches = data
        .get("matches")
        .and_then(Value::as_array)
        .context("The brain answered without a match list")?
        .iter()
        .map(parse_match)
        .collect::<Result<Vec<_>>>()?;

    let verified_against = data
        .get("verified_against")
        .and_then(Value::as_object)
        .map(|roots| {
            roots
                .iter()
                .filter_map(|(memory_type, root)| {
                    Some((
                        MemoryType::from_str(memory_type).ok()?,
                        root.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(EvidenceBundle {
        matches,
        verified_against,
        truncated: data
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        all_verified: data
            .get("all_verified")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        plan: data
            .get("plan")
            .filter(|plan| !plan.is_null())
            .map(parse_plan),
        warnings: envelope.warnings.clone(),
    })
}

fn parse_match(value: &Value) -> Result<EvidenceMatch> {
    let block_id = value
        .get("block_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("A match carried no block identity"))?
        .to_string();
    let memory_type = value
        .get("memory_type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Match {block_id} named no memory type"))?;
    let memory_type = MemoryType::from_str(memory_type)
        .map_err(|error| anyhow!("Match {block_id}: {}", error.message))?;
    let score = value
        .get("score")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let sources = value
        .get("sources")
        .and_then(Value::as_array)
        .map(|sources| {
            sources
                .iter()
                .filter_map(|source| {
                    Some(EvidenceSource {
                        block_id: source.get("block_id")?.as_str()?.to_string(),
                        locator: source
                            .get("locator")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(EvidenceMatch {
        block_id,
        memory_type,
        content: value.get("content").cloned().unwrap_or(Value::Null),
        score,
        sources,
        verified: value
            .get("verified")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        resolvable: value
            .get("resolvable")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        superseded_by: value
            .get("superseded_by")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

fn parse_plan(plan: &Value) -> RetrievalPlan {
    RetrievalPlan {
        signature: plan
            .get("signature")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        intent: plan
            .get("intent")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        indices_consulted: string_list(plan.get("indices_consulted")),
        degradations: plan
            .get("degradations")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(describe_degradation).collect())
            .unwrap_or_default(),
    }
}

/// Flattens one degradation into the sentence the model will read.
fn describe_degradation(value: &Value) -> String {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("degraded");
    match value.get("detail").and_then(Value::as_str) {
        Some(detail) if !detail.is_empty() => format!("{kind} — {detail}"),
        _ => kind.to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/knowledge.rs"]
mod tests;
