//! Unit tests for the Vitruvio command-line brain client.
//!
//! The brain is stubbed as a shell script so the whole contract under test —
//! which flags were passed, what the envelope said, which exit code came back —
//! is visible in one place and no Vitruvio installation is required.

use super::*;

use std::{fs, io::Write, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};

use sfumato_core::{
    errors::ErrorCode,
    knowledge::{BrainBinding, RetrievalMode},
    operation::DiscardEvents,
};
use tempfile::TempDir;

/// A stub brain that records its arguments and replies with a fixed envelope.
struct StubBrain {
    home: TempDir,
}

impl StubBrain {
    /// Builds a brain that prints `stdout`, then exits with `code`.
    fn new(stdout: &str, stderr: &str, code: i32) -> Self {
        let home = TempDir::new().expect("a temporary directory");
        let script = home.path().join("vitruvio");
        let body = format!(
            "#!/bin/sh\nprintf '%s' \"$*\" > \"$(dirname \"$0\")/arguments\"\n\
             cat <<'STDOUT'\n{stdout}\nSTDOUT\n>&2 printf '%s' '{stderr}'\nexit {code}\n"
        );
        // Written through an explicit handle that is synced and closed before the
        // mode is set, rather than through `fs::write`: Linux refuses to exec a
        // file that any process still holds a write descriptor to, and returns
        // ETXTBSY. The suite runs in parallel and several tests spawn processes, so
        // a sibling's fork can be holding an inherited copy of this descriptor
        // across its own exec window. It is a narrow race and it was reached on CI
        // — "Text file busy" out of a test that passes in isolation every time.
        let mut handle = fs::File::create(&script).expect("the stub is created");
        handle
            .write_all(body.as_bytes())
            .expect("the stub is written");
        handle.sync_all().expect("the stub reaches disk");
        drop(handle);
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
        wait_until_executable(&script);
        Self { home }
    }

    fn ok(data: &str) -> Self {
        Self::new(
            &format!(
                r#"{{"vitruvio":"0.1.0","command":"query.search","ok":true,"data":{data},"warnings":[],"error":null}}"#
            ),
            "",
            0,
        )
    }

    fn binding(&self) -> BrainBinding {
        BrainBinding {
            project: None,
            brain: "algebra".to_string(),
            config_file: None,
            executable: Some(self.home.path().join("vitruvio")),
            actor: None,
            timeout_seconds: 10,
        }
    }

    /// What the last invocation was called with.
    fn arguments(&self) -> String {
        fs::read_to_string(self.home.path().join("arguments")).expect("the stub recorded its call")
    }
}

/// Blocks until the stub can actually be executed.
///
/// Turns the ETXTBSY race described above into a short wait instead of a failed
/// test. The window is microseconds; the bound exists so a genuinely broken stub
/// fails rather than hangs.
fn wait_until_executable(script: &std::path::Path) {
    for _ in 0..100 {
        match std::process::Command::new(script).output() {
            Err(error) if error.raw_os_error() == Some(26) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => return,
        }
    }
    panic!("the stub never became executable: {}", script.display());
}

fn search_request(binding: BrainBinding) -> BrainSearchRequest {
    BrainSearchRequest {
        binding,
        question: "how does Jacobi converge".to_string(),
        memory_types: Vec::new(),
        subject: None,
        tags: Vec::new(),
        since: None,
        until: None,
        include_superseded: false,
        mode: None,
        limit: 10,
        expand_depth: 0,
    }
}

async fn search(_brain: &StubBrain, request: BrainSearchRequest) -> SfumatoResult<EvidenceBundle> {
    let (_, operation) = OperationContext::create(None, Arc::new(DiscardEvents));
    VitruvioCliBrainClient
        .search(request, &operation, OperationStage::Draft)
        .await
}

#[tokio::test]
async fn the_search_command_carries_every_filter_the_model_supplied() {
    let brain = StubBrain::ok(r#"{"matches":[],"verified_against":{},"truncated":false}"#);
    let request = BrainSearchRequest {
        memory_types: vec![MemoryType::Canonical, MemoryType::Semantic],
        subject: Some("algebra".to_string()),
        tags: vec!["iterative".to_string()],
        since: Some("2026-01-01T00:00:00Z".to_string()),
        until: Some("2026-06-01T00:00:00Z".to_string()),
        include_superseded: true,
        mode: Some(RetrievalMode::Semantic),
        limit: 7,
        expand_depth: 2,
        ..search_request(brain.binding())
    };

    search(&brain, request).await.expect("the stub answers");

    let arguments = brain.arguments();
    for expected in [
        "--brain algebra",
        "query search how does Jacobi converge",
        "--memory-type canonical",
        "--memory-type semantic",
        "--subject algebra",
        "--tag iterative",
        "--since 2026-01-01T00:00:00Z",
        "--until 2026-06-01T00:00:00Z",
        "--include-superseded",
        "--mode semantic",
        "--limit 7",
        "--expand-depth 2",
    ] {
        assert!(
            arguments.contains(expected),
            "'{expected}' is missing from: {arguments}"
        );
    }
}

#[tokio::test]
async fn the_actor_kind_is_always_agent() {
    // A brain that records who asked must not be told a human did: every query
    // Sfumato makes is made by a model.
    let brain = StubBrain::ok(r#"{"matches":[],"verified_against":{},"truncated":false}"#);

    search(&brain, search_request(brain.binding()))
        .await
        .expect("the stub answers");

    assert!(brain.arguments().contains("--actor-kind agent"));
}

#[tokio::test]
async fn a_configured_project_is_stated_alongside_the_brain() {
    // Vitruvio resolves a project first and a brain within it, and it will fall
    // back to a saved per-project selection when neither is stated. An agent
    // must not depend on that layer: what a run reads has to be decided by the
    // project file, not by wherever the process happened to be started.
    let brain = StubBrain::ok(r#"{"matches":[],"verified_against":{},"truncated":false}"#);
    let mut binding = brain.binding();
    binding.project = Some("facultad".to_string());

    search(&brain, search_request(binding))
        .await
        .expect("the stub answers");

    let arguments = brain.arguments();
    assert!(
        arguments.contains("--project facultad"),
        "the project is missing from: {arguments}"
    );
    assert!(
        arguments.contains("--brain algebra"),
        "the brain is missing from: {arguments}"
    );
}

#[tokio::test]
async fn a_project_this_machine_does_not_know_says_how_to_register_it() {
    // Exit 3 is also "the configuration is invalid", and only this one is fixed
    // by running a command rather than by editing a file.
    let brain = StubBrain::new(
        r#"{"vitruvio":"0.1.0","command":"query.search","ok":false,"data":{},"warnings":[],
            "error":{"code":"PROJECT_NOT_KNOWN","kind":"config",
                     "message":"no project named 'facultad' is registered"}}"#,
        "",
        3,
    );
    let mut binding = brain.binding();
    binding.project = Some("facultad".to_string());

    let error = search(&brain, search_request(binding))
        .await
        .expect_err("an unregistered project is an error");

    assert_eq!(error.code, ErrorCode::Config);
    assert!(
        error.message.contains("vitruvio project register"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn provenance_survives_the_round_trip() {
    let brain = StubBrain::ok(
        r#"{"matches":[{"block_id":"sha256:aa","memory_type":"canonical",
             "content":{"statement":"Jacobi converges under diagonal dominance"},
             "score":"0.87","sources":[{"block_id":"sha256:bb","locator":"lines:1-5"}],
             "verified":true,"resolvable":true,"superseded_by":"sha256:cc"}],
           "verified_against":{"canonical":"sha256:root"},"truncated":false,"all_verified":false}"#,
    );

    let bundle = search(&brain, search_request(brain.binding()))
        .await
        .expect("the stub answers");

    let matched = &bundle.matches[0];
    assert_eq!(matched.block_id, "sha256:aa");
    assert_eq!(matched.memory_type, MemoryType::Canonical);
    // Kept as the string the brain printed: it is agreement between retrieval
    // strategies, and a float would invite presenting it as a confidence.
    assert_eq!(matched.score, "0.87");
    assert_eq!(matched.sources[0].locator.as_deref(), Some("lines:1-5"));
    assert_eq!(matched.superseded_by.as_deref(), Some("sha256:cc"));
    assert_eq!(
        bundle.verified_against.get(&MemoryType::Canonical).unwrap(),
        "sha256:root"
    );
    assert!(!bundle.all_verified);
}

#[tokio::test]
async fn a_truncated_bundle_says_so() {
    let brain = StubBrain::ok(r#"{"matches":[],"verified_against":{},"truncated":true}"#);

    let bundle = search(&brain, search_request(brain.binding()))
        .await
        .expect("the stub answers");

    assert!(bundle.truncated);
}

#[tokio::test]
async fn plan_degradations_reach_the_caller() {
    let brain = StubBrain::ok(
        r#"{"matches":[],"verified_against":{},"truncated":false,
            "plan":{"signature":"abc","intent":"semantic","indices_consulted":["term"],
                    "degradations":[{"kind":"index_absent","detail":"no vector index is installed"}]}}"#,
    );

    let bundle = search(&brain, search_request(brain.binding()))
        .await
        .expect("the stub answers");

    let plan = bundle.plan.expect("the plan is carried");
    assert_eq!(plan.intent.as_deref(), Some("semantic"));
    assert_eq!(
        plan.degradations,
        vec!["index_absent — no vector index is installed".to_string()]
    );
}

#[tokio::test]
async fn envelope_warnings_are_carried_not_swallowed() {
    let brain = StubBrain::new(
        r#"{"vitruvio":"0.1.0","command":"query.search","ok":true,
            "data":{"matches":[],"verified_against":{},"truncated":false},
            "warnings":["statistics are stale"],"error":null}"#,
        "",
        0,
    );

    let bundle = search(&brain, search_request(brain.binding()))
        .await
        .expect("the stub answers");

    assert_eq!(bundle.warnings, vec!["statistics are stale".to_string()]);
}

#[tokio::test]
async fn a_usage_exit_is_permanent_so_the_model_rephrases_instead_of_retrying() {
    let brain = StubBrain::new(
        r#"{"vitruvio":"0.1.0","command":"query.search","ok":false,"data":{},"warnings":[],
            "error":{"code":"USAGE","kind":"usage","message":"--mode is not a memory type",
                     "hint":"Pass --memory-type instead."}}"#,
        "",
        2,
    );

    let error = search(&brain, search_request(brain.binding()))
        .await
        .expect_err("a usage failure is an error");

    assert_eq!(error.code, ErrorCode::Tool);
    assert_eq!(error.class, ErrorClass::Permanent);
    assert!(error.message.contains("USAGE"), "{}", error.message);
    assert!(
        error.message.contains("Pass --memory-type instead."),
        "the brain's own hint is the most actionable part: {}",
        error.message
    );
}

#[tokio::test]
async fn an_internal_exit_is_unavailable_so_it_can_be_retried() {
    let brain = StubBrain::new(
        r#"{"vitruvio":"0.1.0","command":"query.search","ok":false,"data":{},"warnings":[],
            "error":{"code":"INTERNAL","kind":"internal","message":"the index file is locked"}}"#,
        "",
        1,
    );

    let error = search(&brain, search_request(brain.binding()))
        .await
        .expect_err("an internal failure is an error");

    assert_eq!(error.class, ErrorClass::Unavailable);
}

#[tokio::test]
async fn a_configuration_exit_names_the_keys_that_fix_it() {
    let brain = StubBrain::new(
        r#"{"vitruvio":"0.1.0","command":"query.search","ok":false,"data":{},"warnings":[],
            "error":{"code":"CONFIG","kind":"config","message":"no brain named 'algebra'"}}"#,
        "",
        3,
    );

    let error = search(&brain, search_request(brain.binding()))
        .await
        .expect_err("a configuration failure is an error");

    assert_eq!(error.code, ErrorCode::Config);
    for key in ["knowledge.project", "knowledge.brain", "knowledge.config"] {
        assert!(error.message.contains(key), "{key}: {}", error.message);
    }
}

#[tokio::test]
async fn a_missing_executable_says_how_to_install_it() {
    let mut binding = StubBrain::ok("{}").binding();
    binding.executable = Some(PathBuf::from("/nowhere/vitruvio"));
    let (_, operation) = OperationContext::create(None, Arc::new(DiscardEvents));

    let error = VitruvioCliBrainClient
        .search(search_request(binding), &operation, OperationStage::Draft)
        .await
        .expect_err("a brain that is not installed cannot answer");

    assert_eq!(error.code, ErrorCode::Config);
    assert!(
        error.message.contains("knowledge.executable"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn non_json_stdout_is_reported_with_the_stderr_tail() {
    let brain = StubBrain::new(
        "Traceback (most recent call last):",
        "ImportError: usearch",
        1,
    );

    let error = search(&brain, search_request(brain.binding()))
        .await
        .expect_err("output that is not an envelope is an error");

    assert!(
        error.message.contains("ImportError: usearch"),
        "stderr is the only explanation there is: {}",
        error.message
    );
}

#[tokio::test]
async fn index_statistics_failing_degrades_the_card_instead_of_the_run() {
    // `brain info` succeeds and `index stats` does not, because the stub always
    // answers the same way and an anatomy payload has no `statistics`.
    let brain = StubBrain::ok(
        r#"{"brain":"algebra","travelling_indices":["semantic"],
            "modules":[{"memory_type":"canonical","root":"sha256:aa","block_count":1240,
                        "indices":["term","vector"]}]}"#,
    );
    let (_, operation) = OperationContext::create(None, Arc::new(DiscardEvents));

    let card = VitruvioCliBrainClient
        .card(
            BrainCardRequest {
                binding: brain.binding(),
            },
            &operation,
            OperationStage::ReadSources,
        )
        .await
        .expect("a brain with no statistics still describes itself");

    assert_eq!(card.brain, "algebra");
    assert_eq!(card.modules[0].block_count, 1240);
    assert_eq!(card.modules[0].indices, vec!["term", "vector"]);
    assert_eq!(card.travelling_indices, vec!["semantic"]);
}

#[test]
fn a_facet_is_read_from_a_bare_count_or_from_a_value_list() {
    // Vitruvio reports a column as a count today and may report its frequent
    // values later. Reading both means the card names real subjects the day
    // that lands, with no release here.
    let counted = parse_facet("subject", &serde_json::json!(7));
    assert_eq!(counted.distinct, 7);
    assert!(counted.top.is_empty());

    let enumerated = parse_facet(
        "subject",
        &serde_json::json!({"distinct": 2, "top": [["algebra", 120], ["redes", 8]]}),
    );
    assert_eq!(enumerated.distinct, 2);
    assert_eq!(
        enumerated.top,
        vec![("algebra".to_string(), 120), ("redes".to_string(), 8)]
    );
}

#[tokio::test]
async fn a_hanging_brain_is_stopped_by_the_configured_timeout() {
    let home = TempDir::new().expect("a temporary directory");
    let script = home.path().join("vitruvio");
    fs::write(&script, "#!/bin/sh\nsleep 30\n").expect("the stub is written");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("it is executable");
    let binding = BrainBinding {
        project: None,
        brain: "algebra".to_string(),
        config_file: None,
        executable: Some(script),
        actor: None,
        timeout_seconds: 1,
    };
    let (_, operation) = OperationContext::create(None, Arc::new(DiscardEvents));

    let error = VitruvioCliBrainClient
        .search(search_request(binding), &operation, OperationStage::Draft)
        .await
        .expect_err("a brain that never answers must not hang the run");

    assert_eq!(error.class, ErrorClass::Unavailable);
}
