# v0.2 Architecture Traceability

**Status:** implemented. Each row names current code and executable evidence.

| ID | Requirement | Current anchor | Evidence |
| --- | --- | --- | --- |
| V02-001 | Four workspace packages with inward dependency direction. | Root workspace manifest and three crate manifests. | Workspace build and strict Clippy gate. |
| V02-002 | Pure domain types and invariants. | `sfumato-domain::{artifact,deck,primitives,review}`. | Domain unit suite and dependency manifest. |
| V02-003 | Traits only at replaceable outbound boundaries. | Core artifact, config, filesystem, prompt, provider, renderer, repository, source, and tool ports. | Adapter and fake implementations compile against typed contracts. |
| V02-004 | Strict layered prompts with deterministic provenance. | `PromptId`, `PromptCatalog`, `LayeredPromptCatalog`, prompt assets. | `tests/prompts.rs`: parity, strict render, precedence, traversal, symlink, size, customization, aggregate hash. |
| V02-005 | One cancellation-aware generation use case. | `SfumatoApplication::generate_slides`, slide stage modules. | Draft compaction, structural repair, Mermaid repair, review, layout, rendering, and publication tests. |
| V02-006 | Guarded content-only editing. | `SfumatoApplication::edit_slides`, `SlideDeckDocument`. | Patch allowlist, structural rejection, revision guard, focused replacement, and preservation tests. |
| V02-007 | Strict schema-v4 config and indirect secrets. | Adapter DTOs/stores, `SecretRef`, `EffectiveConfig`. | Round trip, side-effect-free read, legacy/future rejection, stale revision, concurrent edit, and redaction tests. |
| V02-008 | Stable typed application errors. | `ErrorCode`, `ErrorClass`, `OperationStage`, `SfumatoError`. | Error contract tests; `sfumato-core` contains no `anyhow` dependency or usage. |
| V02-009 | Cancellation/deadlines through provider, tools, processes, and retries. | `OperationContext`, `AgentRunner`, adapter runtime guards, bounded TUI jobs. | Agent cancellation, runtime deadline, process termination, and stale-job tests. |
| V02-010 | Immutable transactional artifacts. | `ArtifactStore`, `FilesystemArtifactStore`, versioned resource layout. | Commit, drop rollback, pointer rollback, symlink/path rejection, duplicate/reserved path, and concurrent commit tests. |
| V02-011 | Publication cannot invalidate a committed revision. | `resources::slides::publishing`. | Publication failure warning, stale-PDF removal, and non-fatal cleanup tests. |
| V02-012 | CLI and TUI share one facade. | `SfumatoApplication`; root composition; TUI model/reducer/effects/view. | CLI/TUI generation argument parity and presentation tests. |
| V02-013 | Layered deterministic tests and failure recovery. | Domain, core, adapter, and root test suites. | `cargo test --workspace`; fake ports by default, real renderers feature-gated. |
| V02-014 | Prompt overrides cannot weaken code-owned invariants. | Prompt adapter plus deck/patch/path/artifact validators. | Invalid overrides fail before providers; invalid model output cannot commit. |
| V02-015 | Invalid drafts receive bounded focused recovery. | Validation and Mermaid repair prompt pairs and slide orchestration. | One structural repair test, focused Mermaid JSON Patch test, and negative output-contract transcripts. |

Material contract changes require an ADR amendment, a traceability update, and a
test that fails against the previous behavior.
