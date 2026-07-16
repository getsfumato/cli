# v0.2 Testing Contract

**Target status:** Approved for v0.2. **Implementation status:** Mixed. Domain,
prompt resolution, provider transcripts, configuration rejection, and artifact
commit/rollback have executable coverage. Full cancellation propagation,
failure injection, and facade contract suites remain to be completed.

## Test Layers

| Layer | Scope | External I/O |
| --- | --- | --- |
| Domain unit | Invariants, parsing, patches, revisions, path/value validation. | None. |
| Application unit | Use-case sequencing with in-memory ports and a controlled cancellation token. | None. |
| Port contract | One reusable behavior suite per port, run against every adapter. | Adapter-local only. |
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
- Edit patch allowlist, title/frontmatter/ID/order protection, no-op patch,
  stale revision, stale destination, and unrelated-content preservation.
- Complete response enforcement; truncated model output never commits.

### Cancellation

Trigger cancellation deterministically before and after each external call,
between retries/tool rounds, before prepare, after prepare, and after commit
intent. Assert child processes are reaped, no new work starts, events terminate
once, and artifact state follows ADR-0004.

### Artifact transactions

Inject failure before and during staging validation, revision-directory rename,
`current.json` replacement, and processed-output publication. Assert uncommitted
staging rollback, immutable committed revisions, digest checking, cleanup, and
publication-warning semantics. Run edit cases with parent revisions and
generation cases with nested images and diagrams.

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

CI for the parent integration must run formatting, workspace tests, clippy with
warnings denied, and an import-boundary check for ADR-0001. Each implemented row
in [Traceability](../architecture/traceability.md) must name at least one test.
Tests for proposed behavior may land first but must remain clearly named as
target or pending until the corresponding implementation is integrated.
