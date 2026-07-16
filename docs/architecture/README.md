# Sfumato v0.2 Architecture

**Status:** implemented v0.2 reference.

Sfumato separates pure resource rules, application orchestration, infrastructure,
and terminal presentation. The diagrams and references in this directory map to
the current Rust workspace rather than a future target.

## Workspace Shape

Dependencies point inward:

```text
sfumato presentation/composition -> sfumato-core application/ports -> sfumato-domain
                  |                         ^
                  v                         |
         sfumato-adapters ------------------+
```

- **`sfumato-domain`** owns validated identifiers, capabilities, secret
  references, deck parsing, review invariants, revisions, and artifact manifests.
  It has no filesystem, HTTP, process, async-runtime, or terminal dependency.
- **`sfumato-core`** owns `SfumatoApplication`, use cases, provider-neutral agent
  execution, typed errors, cancellation, prompt contracts, repository ports, and
  generation/edit decisions. It has no `anyhow`, Clap, Ratatui, Inquire,
  Indicatif, Reqwest, or process dependency.
- **`sfumato-adapters`** implements schema-v4 TOML persistence, layered
  MiniJinja prompts, filesystem repositories, OpenAI-compatible transports,
  source/tools, Marp, Mermaid, browser inspection, and artifact transactions.
- **`sfumato`** is the composition and presentation package. CLI and TUI both
  receive one production `SfumatoApplication` and translate typed DTOs/events to
  human or JSON output.

Cargo workspace dependencies enforce this direction.

## Views

- [Containers](containers.mmd): crate, frontend, port, and adapter boundaries.
- [Domain model](domain-model.mmd): projects, models, decks, prompts, errors, and artifacts.
- [Prompt resolution](prompt-resolution.mmd): project/user/bundled precedence and provenance.
- [Generation sequence](generation-sequence.mmd): draft, repair, review, render, commit, and publish.
- [Edit sequence](edit-sequence.mmd): revision-guarded content-only editing.
- [Config lifecycle](config-lifecycle.mmd): strict v4 reads and atomic revision-aware writes.
- [Artifact transaction](artifact-transaction.mmd): staging, validation, immutable commit, and publication.
- [Traceability](traceability.md): requirements mapped to code and executable evidence.

The [class diagram](../class-diagram.mmd) contains concrete Rust types and trait
relationships. Accepted architectural decisions live under [`docs/adr`](../adr/),
and operator/integrator contracts under [`docs/reference`](../reference/).

## Invariants

1. Domain and application behavior do not depend on terminal, HTTP, process, or
   filesystem implementations.
2. Prompt text may change pedagogy, but cannot change validators, patch policy,
   retry limits, path containment, tool policy, or artifact rules.
3. Every core exit is a classified `SfumatoError` with stable code, class,
   retryability, optional stage, sanitized message, and structured details.
4. Cancellation and deadlines are checked between provider turns, tool calls,
   renderers, retries, and artifact stages. Child processes are terminated by
   adapter runtime guards.
5. Review and edit changes are RFC 6902 patches guarded by document and slide
   revisions. Editing cannot replace frontmatter, title, IDs, or slide order.
6. Generation writes only to transaction staging. A revision becomes visible
   after manifest validation and atomic commit; `current.json` changes last.
7. Publication happens after commit. Publication failure is a warning and a run
   without a PDF removes any stale published PDF.
8. Prompt provenance records ID, origin, manifest version, and SHA-256 source
   hash in the generation result and committed manifest metadata.
9. Mermaid fences are rendered with project theme tokens and embedded at a
   bounded 300 px Marp height in both layout previews and final artifacts, so
   intrinsic SVG dimensions cannot create undetected overflow.

## Verification Gate

The implementation is accepted only when all of these commands pass:

```text
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo doc --workspace --no-deps
```

Production renderer and network checks remain explicit end-to-end verification,
not prerequisites for deterministic unit tests.
