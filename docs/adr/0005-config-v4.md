# ADR-0005: Resolve Strict Config v4 Into Immutable Snapshots

**Decision status:** Accepted and implemented for v0.2.

## Context

Configuration spans global preferences, a project registry, portable project
settings, and command overrides. v0.2 adds validated domain names, indirect
secret references, and operation-level consistency.

## Decision

All three TOML documents use `schema_version = 4` and deny unknown fields.
Connector API keys become optional indirect `SecretRef` values such as
`stored:connector/openrouter` and `env:OPENROUTER_API_KEY`; raw secret material
is invalid in v4. Interactive local setup defaults to the native operating
system credential store. Environment references remain available for CI.

Core exposes `SecretResolver` for provider requests and `SecretStore` for
credential-management use cases. The OS keyring is an adapter, not a core
concept. A future server can replace it with a tenant-scoped cloud secret
manager without changing connector or generation workflows.

Prompt roots are conventional directories rather than config fields: user
overrides live under the platform config directory's `sfumato/prompts`, project
overrides under `<project-root>/.sfumato/prompts`, and project templates win.

Load each scope without mutation. Legacy, missing, and future schema versions
fail without writing; v0.2 is an explicit configuration reset. Validate complete
v4 documents before replacing originals through same-directory temporary files.
Writes hold a cross-process lock and reject a stale content revision.

Resolution order remains command over project over global. Validated project,
model-profile, theme, and capability values plus model/connector references,
supported secret schemes, theme adapters, and publication paths are checked
before producing an immutable `EffectiveConfig`. One operation uses one snapshot
even if files change while it runs.

Config mutation is typed: dotted-key commands modify an in-memory v4 document,
validate it, and use the same atomic replacement path.

## Consequences

- Use cases receive validated values and provenance, not mutable repositories.
- Version failure preserves original files byte-for-byte.
- Strict decoding catches misspellings and schema changes require an explicit
  reset or a future, separately designed migration.
- Prompt customization remains portable without config path indirection.

The normative fields and reset rules are in [Config v4](../reference/config-v4.md).
