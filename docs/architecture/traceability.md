# v0.2 Architecture Traceability

**Status:** v0.2 implementation matrix. Mixed rows name the remaining boundary
or verification work directly.

| ID | v0.2 requirement | Decision or view | Current anchor | Status | Required verification |
| --- | --- | --- | --- | --- | --- |
| V02-001 | The workspace separates `sfumato`, `sfumato-core`, `sfumato-domain`, and `sfumato-adapters` with inward dependencies. | [ADR-0001](../adr/0001-layered-core.md), [containers](containers.mmd) | Workspace manifests | Implemented | Workspace build and clippy pass. |
| V02-002 | Domain types and invariants are free of filesystem, HTTP, process, async runtime, and terminal concerns. | [ADR-0001](../adr/0001-layered-core.md), [domain model](domain-model.mmd) | `sfumato-domain` primitives, deck, review, artifact | Implemented | Domain tests and dependency manifest enforce isolation. |
| V02-003 | Traits exist at replaceable outbound boundaries and are owned by their callers. | [ADR-0002](../adr/0002-port-traits.md) | Provider and repository traits | Mixed | Shared contract suite passes for every adapter. |
| V02-004 | Prompt lookup is typed, deterministic, strict, project-over-user-over-bundled, and provenance-bearing. | [ADR-0003](../adr/0003-prompt-resolution.md), [prompt view](prompt-resolution.mmd) | `PromptCatalog`, `LayeredPromptCatalog`, bundled assets | Implemented | Every ID renders; precedence, invalid override, limits, traversal, and symlink tests pass. |
| V02-005 | Generation follows one cancellation-aware use-case contract. | [generation sequence](generation-sequence.mmd), [public API](../reference/public-api.md) | `resources/slides.rs` | Mixed | Use-case integration tests cover normal, compact, review, render, and publish paths. |
| V02-006 | Editing accepts guarded content-only patches and preserves structural identity. | [edit sequence](edit-sequence.mmd) | `resources/slides/edit.rs`, `DeckDocument` | Implemented | Guard, structural rejection, and focused replacement tests pass. |
| V02-007 | Config schema v4 uses validated domain names, indirect secrets, a deliberate legacy reset, and immutable effective snapshots. | [ADR-0005](../adr/0005-config-v4.md), [config lifecycle](config-lifecycle.mmd) | `config.rs`, `config_editor.rs`, `SecretRef` | Implemented | Round-trip plus legacy/future read-only rejection tests pass. |
| V02-008 | Public operations expose stable typed errors with code, stage, and retryability. | [ADR-0004](../adr/0004-errors-and-cancellation.md) | `errors.rs` | Mixed | Typed contract tests pass; remaining `anyhow` workflow exits still need mapping at the facade. |
| V02-009 | Cancellation propagates through providers, tools, processes, retries, and transactions. | [ADR-0004](../adr/0004-errors-and-cancellation.md) | `operation.rs`, bounded TUI jobs | Mixed | Context/TUI tests pass; process adapters still need cooperative termination. |
| V02-010 | Generation and edit artifacts use an immutable staging transaction. | [ADR-0006](../adr/0006-artifact-transactions.md), [transaction view](artifact-transaction.mmd) | `ArtifactStore`, `FilesystemArtifactStore` | Implemented | Commit, rollback, path escape, and manifest validation tests pass. |
| V02-011 | Publication is downstream of workspace commit and cannot invalidate committed workspace artifacts. | [ADR-0006](../adr/0006-artifact-transactions.md) | PDF copy after generation | Mixed | Publication failure returns warning and committed receipts. |
| V02-012 | CLI, TUI, and Rust callers share the same application requests, results, errors, events, and cancellation. | [public API](../reference/public-api.md), [containers](containers.mmd) | CLI/TUI call core functions directly | Proposed | Presenter parity tests and API compile tests. |
| V02-013 | Tests are layered, include port contracts, and exercise failure recovery. | [testing reference](../reference/testing.md) | Existing unit and integration suites | Mixed | CI runs the v0.2 test matrix. |
| V02-014 | Prompt overrides are contained UTF-8 MiniJinja templates; output invariants remain code-enforced. | [prompt authoring](../reference/prompt-authoring.md), [ADR-0003](../adr/0003-prompt-resolution.md) | Layered prompt adapter and `SFUMATO.md` input | Implemented | Strict variable, traversal, symlink, size, precedence, and deck invariant tests pass. |

## Completion Rule

Update a row to **Implemented** only when both behavior and verification named
in the final column are present. Amend the relevant ADR when implementation
chooses a materially different contract.
