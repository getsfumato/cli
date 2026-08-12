//! Unit tests for the knowledge-source port and its configuration.

use std::path::PathBuf;

use super::{MemoryType, RetrievalMode, refuse_sources_under_brain};
use crate::config::{KnowledgeBackend, KnowledgeConfig, ProjectConfig};

fn brain_project(knowledge: KnowledgeConfig) -> ProjectConfig {
    ProjectConfig {
        name: "university".to_string(),
        theme: "sfumato-default".to_string(),
        publish_dir: None,
        model_defaults: Default::default(),
        model_roles: Default::default(),
        page: Default::default(),
        generation_tools: Default::default(),
        security: Default::default(),
        knowledge,
        marp: None,
    }
}

/// A global layer that configures nothing, for the tests that only care about
/// what the project's `[knowledge]` table resolves to.
fn bare_global() -> crate::config::GlobalConfig {
    crate::config::GlobalConfig {
        user: crate::config::UserConfig {
            name: None,
            learning_style: Vec::new(),
        },
        connectors: Default::default(),
        models: Default::default(),
        defaults: Default::default(),
        model_roles: Default::default(),
        marp: crate::config::MarpConfig {
            pdf: false,
            browser_path: None,
        },
        browser: Default::default(),
    }
}

#[test]
fn a_vitruvio_project_without_a_brain_is_rejected_at_validation() {
    let project = brain_project(KnowledgeConfig {
        backend: KnowledgeBackend::Vitruvio,
        ..KnowledgeConfig::default()
    });

    let error = project.validate().expect_err("a nameless brain is invalid");

    assert!(
        error.to_string().contains("knowledge.brain"),
        "the error must name the key that fixes it: {error}"
    );
}

#[test]
fn a_named_brain_validates() {
    let project = brain_project(KnowledgeConfig {
        backend: KnowledgeBackend::Vitruvio,
        brain: Some("algebra".to_string()),
        ..KnowledgeConfig::default()
    });

    project.validate().expect("a named brain is valid");
}

#[test]
fn naming_a_project_and_a_configuration_file_at_once_is_rejected() {
    // Vitruvio takes `--config` verbatim and stops asking, so the project name
    // would be inert. A key that quietly does nothing is worse than a refusal.
    let project = brain_project(KnowledgeConfig {
        backend: KnowledgeBackend::Vitruvio,
        project: Some("facultad".to_string()),
        brain: Some("algebra".to_string()),
        config_file: Some(PathBuf::from("../vitruvio/vitruvio.toml")),
        ..KnowledgeConfig::default()
    });

    let error = project
        .validate()
        .expect_err("two ways to name one project is one too many");

    assert!(
        error.to_string().contains("knowledge.project"),
        "the error must name the keys that collide: {error}"
    );
}

#[test]
fn a_named_project_reaches_the_binding() {
    let project = brain_project(KnowledgeConfig {
        backend: KnowledgeBackend::Vitruvio,
        project: Some("  facultad  ".to_string()),
        brain: Some("algebra".to_string()),
        ..KnowledgeConfig::default()
    });
    let config = crate::config::EffectiveConfig::from_parts(
        bare_global(),
        project.name.clone(),
        PathBuf::from("/projects/university"),
        project,
        Default::default(),
    )
    .expect("the project is valid");

    let binding = config.brain_binding().expect("it is brain-backed");

    assert_eq!(binding.project.as_deref(), Some("facultad"));
    assert_eq!(binding.brain, "algebra");
}

#[test]
fn a_project_with_no_knowledge_table_reads_as_filesystem() {
    let knowledge = KnowledgeConfig::default();

    assert_eq!(knowledge.backend, KnowledgeBackend::Filesystem);
    assert!(!knowledge.uses_brain());
    brain_project(knowledge)
        .validate()
        .expect("the default grounding is valid");
}

#[test]
fn a_maximum_below_the_default_limit_is_rejected() {
    let knowledge = KnowledgeConfig {
        default_limit: 20,
        max_limit: 10,
        ..KnowledgeConfig::default()
    };

    let error = knowledge
        .validate()
        .expect_err("a ceiling below the floor is invalid");

    assert!(error.to_string().contains("knowledge.max_limit"), "{error}");
}

#[test]
fn source_paths_are_refused_rather_than_ignored_under_the_brain_backend() {
    let error = refuse_sources_under_brain("algebra", &[PathBuf::from("notes/jacobi.md")])
        .expect_err("source paths cannot ground a brain-backed project");

    let message = error.to_string();
    assert!(message.contains("algebra"), "{message}");
    assert!(
        message.contains("knowledge.backend"),
        "the refusal must say how to opt back into files: {message}"
    );
}

#[test]
fn a_brain_backed_project_without_source_paths_is_accepted() {
    refuse_sources_under_brain("algebra", &[]).expect("no paths is the normal case");
}

#[test]
fn an_unknown_memory_type_is_refused_with_the_valid_ones_named() {
    let error = "episodes"
        .parse::<MemoryType>()
        .expect_err("'episodes' is not a module");

    let message = error.to_string();
    for memory_type in MemoryType::ALL {
        assert!(
            message.contains(memory_type.as_str()),
            "'{}' must appear in the refusal: {message}",
            memory_type.as_str()
        );
    }
}

#[test]
fn memory_types_and_modes_parse_their_own_wire_names() {
    for memory_type in MemoryType::ALL {
        assert_eq!(
            memory_type.as_str().parse::<MemoryType>().expect("parses"),
            memory_type
        );
    }
    for mode in RetrievalMode::ALL {
        assert_eq!(
            mode.as_str().parse::<RetrievalMode>().expect("parses"),
            mode
        );
    }
}

// --- the brain card and the tool-less retry ---------------------------------

use crate::{
    knowledge::{
        BrainCard, BrainEvidenceRecord, BrainFacet, BrainModule, EvidenceBundle, EvidenceMatch,
    },
    resources::{build_brain_card, build_compact_evidence_bundle},
};

fn module(memory_type: MemoryType, blocks: u64, indices: &[&str]) -> BrainModule {
    BrainModule {
        memory_type,
        block_count: blocks,
        resolvable: Some(blocks),
        root: Some("sha256:aa".to_string()),
        indices: indices.iter().map(ToString::to_string).collect(),
        freshness: Some("fresh".to_string()),
    }
}

#[test]
fn the_card_names_every_installed_module_and_its_block_count() {
    let card = BrainCard {
        brain: "algebra".to_string(),
        modules: vec![
            module(MemoryType::Canonical, 1240, &["term", "vector"]),
            module(MemoryType::Semantic, 318, &["term"]),
        ],
        ..BrainCard::default()
    };

    let rendered = build_brain_card(&card);

    assert!(rendered.contains("algebra"), "{rendered}");
    assert!(rendered.contains("canonical"), "{rendered}");
    assert!(rendered.contains("1240"), "{rendered}");
    assert!(rendered.contains("semantic"), "{rendered}");
    assert!(rendered.contains("318"), "{rendered}");
}

#[test]
fn a_facet_with_no_enumerable_values_says_how_many_there_are() {
    // Vitruvio reports a column as a count today. Saying "7 values" is honest;
    // inventing examples to fill the line would not be.
    let card = BrainCard {
        brain: "algebra".to_string(),
        modules: vec![module(MemoryType::Canonical, 10, &["facet"])],
        facets: vec![BrainFacet {
            name: "subject".to_string(),
            distinct: 7,
            top: Vec::new(),
        }],
        ..BrainCard::default()
    };

    let rendered = build_brain_card(&card);

    assert!(rendered.contains("subject (7 values)"), "{rendered}");
}

#[test]
fn a_facet_that_knows_its_values_lists_them() {
    let card = BrainCard {
        brain: "algebra".to_string(),
        modules: vec![module(MemoryType::Canonical, 10, &["facet"])],
        facets: vec![BrainFacet {
            name: "subject".to_string(),
            distinct: 2,
            top: vec![("algebra".to_string(), 120), ("redes".to_string(), 8)],
        }],
        ..BrainCard::default()
    };

    let rendered = build_brain_card(&card);

    assert!(rendered.contains("subject (algebra, redes)"), "{rendered}");
}

#[test]
fn an_unindexed_module_is_named_rather_than_left_to_disappoint() {
    let card = BrainCard {
        brain: "algebra".to_string(),
        modules: vec![module(MemoryType::Episodic, 96, &[])],
        ..BrainCard::default()
    };

    let rendered = build_brain_card(&card);

    assert!(
        rendered.contains("No index is built over episodic"),
        "{rendered}"
    );
}

#[test]
fn a_brain_with_nothing_installed_says_so() {
    let rendered = build_brain_card(&BrainCard {
        brain: "empty".to_string(),
        ..BrainCard::default()
    });

    assert!(rendered.contains("holds nothing"), "{rendered}");
}

fn record(matches: Vec<EvidenceMatch>) -> BrainEvidenceRecord {
    BrainEvidenceRecord {
        question: "how does Jacobi converge".to_string(),
        filters: Vec::new(),
        bundle: EvidenceBundle {
            matches,
            ..EvidenceBundle::default()
        },
    }
}

fn evidence(block_id: &str, verified: bool, superseded: bool) -> EvidenceMatch {
    EvidenceMatch {
        block_id: block_id.to_string(),
        memory_type: MemoryType::Canonical,
        content: serde_json::json!({"statement": "Jacobi converges under diagonal dominance"}),
        score: "0.9".to_string(),
        sources: Vec::new(),
        verified,
        resolvable: true,
        superseded_by: superseded.then(|| "sha256:cc".to_string()),
    }
}

#[test]
fn the_compact_bundle_drops_unverified_and_superseded_evidence() {
    // The prompt forbids writing from either, so spending the retry's character
    // budget on them would push out material the model may actually use.
    let records = vec![record(vec![
        evidence("sha256:aa", true, false),
        evidence("sha256:bb", false, false),
        evidence("sha256:cc", true, true),
    ])];

    let rendered = build_compact_evidence_bundle(&records, 4_000);

    assert!(rendered.contains("sha256:aa"), "{rendered}");
    assert!(!rendered.contains("sha256:bb"), "{rendered}");
    assert!(!rendered.contains("sha256:cc"), "{rendered}");
    assert!(rendered.contains("1 verified block"), "{rendered}");
}

#[test]
fn the_compact_bundle_names_the_questions_that_produced_it() {
    let rendered =
        build_compact_evidence_bundle(&[record(vec![evidence("sha256:aa", true, false)])], 4_000);

    assert!(rendered.contains("how does Jacobi converge"), "{rendered}");
}

#[test]
fn a_block_seen_twice_is_carried_once() {
    let records = vec![
        record(vec![evidence("sha256:aa", true, false)]),
        record(vec![evidence("sha256:aa", true, false)]),
    ];

    let rendered = build_compact_evidence_bundle(&records, 4_000);

    assert_eq!(rendered.matches("sha256:aa").count(), 1, "{rendered}");
}

#[test]
fn a_retry_with_nothing_retrieved_is_told_to_write_less_rather_than_invent() {
    let rendered = build_compact_evidence_bundle(&[], 4_000);

    assert!(rendered.contains("No usable evidence"), "{rendered}");
    assert!(rendered.contains("cannot ground"), "{rendered}");
}
