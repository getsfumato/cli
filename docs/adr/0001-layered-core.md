# ADR-0001: Layer The Core Around Use Cases

**Decision status:** Accepted for v0.2. **Implementation status:** Mixed. The
workspace and inward dependency graph exist; renderer, source, theme, and config
filesystem implementations still need extraction from `sfumato-core`.

## Context

The current `sfumato-core` exposes useful services, provider traits, structured
review types, and renderer helpers. Slide orchestration also constructs adapters,
reads files, builds prompts, runs processes, and writes artifacts in the same
module. That coupling makes cancellation, fault testing, and a stable library API
hard to introduce without changing presentation clients.

## Decision

Use four workspace crates with inward-facing dependencies:

1. **`sfumato-domain`:** entities, value objects, manifests, review invariants,
   and domain failures. It uses no async runtime or I/O APIs.
2. **`sfumato-core`:** generate, edit, config, setup, project, model, connector,
   and theme use cases plus caller-owned ports.
3. **`sfumato-adapters`:** MiniJinja prompts, filesystem,
   OpenAI-compatible HTTP, Marp, Mermaid, and browser implementations.
4. **`sfumato`:** the composition root and CLI/TUI/JSON presentation.

Presentation may depend on application DTOs. Adapters may depend on ports and
domain value objects. Domain never depends outward. Application does not import
concrete adapters.

The crate graph enforces the boundary: domain depends on neither core nor
adapters; core depends on domain; adapters depend on core/domain; `sfumato`
depends on all three to compose the application.

## Consequences

- CLI, TUI, and library callers execute the same use cases.
- Domain and application tests can run with deterministic in-memory ports.
- Existing behavior must be moved behind boundaries without opportunistic
  feature changes.
- Composition becomes explicit and slightly more verbose.

## Migration

Extract domain types and ports before moving orchestration. Wrap current
filesystem and provider code as adapters, then migrate generation and edit one
stage at a time. Keep temporary compatibility exports private or deprecated;
do not establish a second permanent API.
