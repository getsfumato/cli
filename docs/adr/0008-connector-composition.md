# ADR 0008: Compose Generation Transports With Native Connector APIs

## Status

Accepted.

## Decision

Connector configuration identifies both a generation transport and, when
available, a provider-native adapter. OpenRouter and Ollama compose the shared
OpenAI-compatible transport instead of duplicating chat and image request code.
Codex App Server keeps its native JSON-RPC generation transport.

`ProviderFactory` creates capability-specific generation providers.
`ConnectorIntrospection` independently advertises and executes optional model
catalog, account, usage, and runtime operations. Frontends query advertised
capabilities before presenting an action; unsupported operations return a typed
error rather than being simulated through another protocol.

Legacy schema-v4 OpenAI-compatible entries whose base URLs unambiguously identify
OpenRouter or Ollama receive the corresponding native adapter at runtime. Reads
remain side-effect-free and do not rewrite their configuration.

## Consequences

CLI, TUI, MCP, and future web frontends can share one typed connector contract
while exposing richer provider features. Adding a native endpoint does not alter
model-profile resolution or generation workflows. Native adapters reuse the same
secret reference and never return credentials in status DTOs.

Capabilities describe implemented operations only. New provider endpoints require
an adapter implementation, presentation mapping, tests, and an advertised
capability before frontends expose them.
