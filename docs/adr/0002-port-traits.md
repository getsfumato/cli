# ADR-0002: Put Narrow Traits At Volatile Boundaries

**Decision status:** Accepted for v0.2. **Implementation status:** Mixed. Text,
image, tool, repository, prompt, artifact, and event ports exist; renderer and
source boundaries remain to be extracted.

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

## Compatibility

The existing `TextGenerationProvider`, `ImageGenerationProvider`,
`ToolExecutor`, repository traits, and `PromptCatalog` should be adapted rather
than duplicated.
Their public error and cancellation signatures change only at the v0.2 facade
boundary until internal migration is complete.
