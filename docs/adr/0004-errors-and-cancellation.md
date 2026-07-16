# ADR-0004: Make Errors Typed And Cancellation Cooperative

**Decision status:** Accepted for v0.2. **Implementation status:** Mixed.
Typed errors, operation context, cancellation primitives, and nonblocking event
ports exist; legacy workflow exits and child-process cancellation are not yet
fully mapped.

## Context

CLI users need concise actionable messages, JSON callers need stable codes, and
the TUI needs to distinguish cancellation from failure. Long-running provider,
tool, browser, Mermaid, and Marp operations must stop without publishing partial
artifacts.

## Decision

Public use cases return `Result<T, SfumatoError>`. Each error exposes a stable
`ErrorCode`, operation stage, retryability, user-safe message, and optional
structured details. Categories are config, validation, not found, provider,
tool, render, artifact, cancelled, and internal. Adapter errors remain sources
for diagnostics and are mapped once at their port boundary.

Every use case receives an `OperationContext` containing `JobId`,
cancellation token, and event sink. Cancellation is cooperative and checked:

- before and after every external call;
- between retry attempts and tool rounds;
- before starting an artifact transaction;
- after prepare and immediately before commit.

Provider requests observe the token; spawned child processes are terminated and
reaped; tool loops stop scheduling work. Cancellation before commit rolls back
staging and returns `Cancelled`. Once commit begins, the coordinator completes or
recovers the commit and returns the committed result with a cancellation warning.
This avoids reporting cancellation while leaving an unknown artifact state.

`Cancelled` is not logged as an internal error and is not automatically retried.
Compact recovery remains a response to typed provider limits, not to arbitrary
provider failures.

## Consequences

- All presenters can render the same failure semantics.
- Cancellation tests can target deterministic checkpoints.
- Adapters must preserve useful sources without leaking credentials or response
  bodies into user-facing details.
- Process adapters require explicit kill-and-wait behavior.
