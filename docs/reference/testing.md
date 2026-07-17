# v0.2 Testing Contract

**Status:** Implemented for v0.2. The default workspace suite is deterministic,
offline, and credential-free. Tests that execute real Marp, Mermaid, or browser
binaries are opt-in through the `real-renderers` feature.

## Test Layers

| Layer | Scope | External I/O |
| --- | --- | --- |
| Domain unit | Invariants, parsing, patches, revisions, path/value validation. | None. |
| Application unit | Use-case sequencing with in-memory ports and a controlled cancellation token. | None. |
| Port contract | Provider transcripts and behavior-focused fake implementations. | Adapter-local only. |
| Adapter integration | TOML/filesystem, HTTP mapping, Marp, Mermaid, browser, and process lifecycle. | Temporary directories or explicit local binaries. |
| Facade integration | Composition, DTOs, events, errors, config snapshots, and manifests. | Fakes by default. |
| CLI/TUI presentation | Argument routing, human output, JSON schema, and cancellation controls. | Facade fake. |
| End to end | Selected production-adapter happy paths. | Opt-in; never required network by default. |

Tests use fakes that implement ports and expose meaningful behavior such as
limits, delays, malformed patches, partial writes, and process termination.
Tests do not mock private call graphs.

## Required Suites

### Prompt resolution

- Manifest and `PromptId::all()` parity plus golden output for every ID.
- Project-over-user-over-bundled precedence and deterministic provenance hash.
- Strict required-variable, MiniJinja syntax, and contained-include tests.
- Unknown ID, path escape, symlink escape, invalid UTF-8, oversize template,
  missing override, no-overwrite customization, and redaction cases.
- Structural validators remain effective when an override omits prompt rules.

### Config v4

- Global, registry, project, and effective fixtures.
- Read-only rejection of v1/v2/v3 and future schema versions.
- Unknown field, missing reference, capability mismatch, future version,
  unsupported secret scheme, and prohibited raw-secret edits.
- Faults at temporary write, validation, and atomic replacement; originals must
  remain intact.
- One immutable snapshot per operation despite concurrent file changes.

### Generation and edit

- Normal and compact provider paths, tool-round limits, semantic review,
  corrective retry, focused layout repair, diagram rendering, and dry run.
- Mermaid SVG embedding uses the same bounded Marp dimensions during layout
  inspection and final rendering.
- Edit patch allowlist, title/frontmatter/ID/order protection, no-op patch,
  stale revision, stale destination, and unrelated-content preservation.
- Complete response enforcement; truncated model output never commits.
- Structured page parsing, constrained semantic/browser patches, static HTML,
  CSS, and JavaScript validation, offline CSP assembly, image allowlists, and
  desktop/mobile browser issue scoring.
- Plugin manifest ordering, exact versions, runtime hashes, license presence,
  and opt-in offline Chrome execution for every bundled runtime.
- Structural template marker/path validation and content-only prompt contracts.
- Project artifact integrity, passive SVG policy, revision staging, and model
  reference paths.
- DESIGN.md token round trips and generated renderer adapters.
- UI plugin dependency order and offline React component mounting.

### Cancellation and deadlines

`errors_operation.rs`, `agent_runner.rs`, adapter runtime tests, and TUI lifecycle
tests cover cancellation before provider work, bounded tool rounds, deadlines,
child-process termination, nonblocking events, and stale job completion. New
external-call adapters must add the same checkpoint and process-reaping cases.

### Artifact transactions

`tests/artifacts.rs` injects staging, manifest, pointer, path, symlink, duplicate,
and concurrent-commit failures. Slide publication tests verify warning semantics
and stale-PDF removal. New transaction implementations must preserve the same
rollback and immutable-revision contract.
Page publication additionally tests atomic directory replacement, the stable
`_sfumato/pages/<slug>/` namespace, generated Obsidian indexes, and stale legacy
shape cleanup. Page renderer tests verify conditional MathJax integrity,
offline SVG typesetting, and fatal reporting for unprocessed TeX.

### Public API and presentation

- Compile tests for documented facade usage.
- Serialization snapshots for results, events, and stable error codes.
- CLI and TUI parity: identical requests yield equivalent result semantics.
- Human output sends progress to stderr and machine JSON keeps stdout valid.

## Determinism

Inject job IDs, clock, provider responses, cancellation checkpoints, and
filesystem fault plans. Normalize platform-dependent paths before snapshots.
Do not use sleeps to coordinate async tests; use barriers or explicit fake-port
signals. Default tests do not require API credentials, network access, or locally
installed rendering binaries.

## Fixtures And Goldens

Fixtures are minimal and named by behavior. Golden updates must be reviewed as
contract changes, especially for prompts and serialized DTOs.
Secrets and user home paths must never appear in committed fixtures.

## Architecture Gate

The release gate is:

```text
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo doc --workspace --no-deps
```

`tests/architecture.rs` enforces ADR-0001 independently of compilation.
Each implemented row in [Traceability](../architecture/traceability.md) names
executable evidence. Proposed behavior must be labelled as such until its test
and implementation are integrated.
