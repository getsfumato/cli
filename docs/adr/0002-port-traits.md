# ADR-0002: Put Narrow Traits At Volatile Boundaries

**Decision status:** Accepted and implemented for v0.2.

## Context

Traits are valuable where application policy crosses into replaceable I/O, but
traits for every service obscure ownership and make domain code harder to read.
The current provider traits are useful precedents; direct renderer and filesystem
calls are difficult to cancel and fault-test.

## Decision

The application layer owns outbound port traits. Add a trait only when at least
one of these is true:

- implementations vary by connector, renderer, or storage backend;
- tests need deterministic failure, timing, or cancellation control;
- the operation crosses network, filesystem, clock, process, or event boundaries.

Required v0.2 port families are provider, config/project/theme repository,
source reader, `PromptCatalog`, artifact transaction/store, renderer, layout
inspector, and event sink. Cancellation is carried by `OperationContext`,
not hidden behind a service locator.

Port methods use owned request/result DTOs, return the typed public error model,
and are `Send + Sync` when shared by async use cases. Trait objects are preferred
at composition boundaries; generics remain appropriate inside adapters where
they measurably simplify code or improve performance.

Do not create traits for pure domain calculations, DTO conversion, or a type
with one stable implementation and no test boundary.

## Consequences

- Contract tests can be reused across filesystem and HTTP adapters.
- Mocks model capabilities rather than mirror large concrete services.
- Adding a new adapter does not change use-case orchestration.
- Port evolution must be deliberate because it affects every implementation.

## Current Contract

`TextGenerationProvider`, `ImageGenerationProvider`, `ToolExecutor`, repository
traits, `PromptCatalog`, `SourceReader`, `WorkspaceFileSystem`, renderers, and
the artifact store all return typed core results. Adapters may use richer local
errors internally but classify them once before crossing a port boundary.
