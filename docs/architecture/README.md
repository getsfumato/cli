# Sfumato v0.2 Architecture

**Status:** v0.2 implementation reference.

This directory documents the v0.2 workspace and records remaining boundary work
explicitly in the traceability table.

## Status Vocabulary

| Label | Meaning |
| --- | --- |
| Implemented | Present in the v0.2 workspace with verification. |
| In flight | Present but not yet routed through every public workflow. |
| Proposed | Required by the approved v0.2 target and not yet integrated. |
| Mixed | Existing behavior is retained behind a proposed boundary. |

A row is implemented only when both its behavior and named verification exist.

## Target Shape

Dependencies point inward through the v0.2 workspace crates:

```text
sfumato presentation/composition -> sfumato-core application/ports -> sfumato-domain
                  |                         ^
                  v                         |
         sfumato-adapters ------------------+
```

- **`sfumato-domain`** owns resource models, validated identifiers, manifests,
  review invariants, and domain failures. It has no I/O or async runtime.
- **`sfumato-core`** owns use cases and caller-owned ports, including prompt
  catalog contracts, provider contracts, cancellation, and transaction decisions.
- **`sfumato-adapters`** currently implements prompt templates,
  OpenAI-compatible HTTP, and transactional artifact storage. Renderer and
  configuration adapter extraction remains tracked work.
- **`sfumato`** is the presentation and composition crate. It wires concrete
  adapters and maps shared application DTOs to CLI, TUI, and JSON output.

The crate dependency direction is enforced by Cargo workspace dependencies.

## Views

- [Containers](containers.mmd): runtime boundaries and dependency direction.
- [Domain model](domain-model.mmd): requests, documents, prompts, errors, and artifacts.
- [Prompt resolution](prompt-resolution.mmd): deterministic layered template lookup and provenance.
- [Generation sequence](generation-sequence.mmd): complete generation lifecycle.
- [Edit sequence](edit-sequence.mmd): guarded content-only deck editing.
- [Config lifecycle](config-lifecycle.mmd): strict v4 load, merge, and atomic mutation.
- [Artifact transaction](artifact-transaction.mmd): staging, immutable commit, and publish.
- [Traceability](traceability.md): requirements mapped to decisions and verification.

## Decisions And Contracts

The accepted decisions are in [`docs/adr`](../adr/). Operator and integrator
contracts are in [`docs/reference`](../reference/). The
[class diagram](../class-diagram.mmd) maps to concrete Rust types and ports.

## Non-Negotiable Invariants

1. Domain and application behavior do not depend on terminal, HTTP, process, or
   filesystem implementations.
2. Prompt templates may be overridden, but code-owned validation, patch policy,
   path containment, and artifact rules cannot be overridden by prompt text.
3. A cancellation observed before commit leaves no new final artifacts; a
   cancellation observed after commit returns the committed manifest.
4. Editing preserves title slide, frontmatter, deck title, slide IDs, and order.
5. One immutable revision is committed by renaming its staging directory; the
   `current.json` pointer changes only after revision validation.
6. Public callers receive typed errors and stable result DTOs, not adapter errors
   or internal orchestration types.

## Integration Gate

Boundary cleanup is complete when every mixed or proposed traceability row has
an implementation reference and its required verification passes.
