# ADR 0007: Separate Reusable Generation Inputs

## Status

Accepted.

## Decision

Themes, structural templates, project artifacts, and executable page plugins
remain separate concepts and outbound ports.

- Themes own semantic design tokens and renderer adapters.
- Templates own one validated content slot and no model policy.
- Project artifacts are portable, integrity-checked media copied into each
  resource transaction.
- Page plugins own pinned executable runtimes, dependency metadata, guidance,
  and licenses.

The page plugin catalog resolves dependency graphs. Component libraries use
the generic plugin selector (`--plugin`, with `--ui` as an alias) rather than
adding one CLI flag or Rust command variant per library.

## Consequences

Draft models return generated content rather than repeating selected templates.
Generated resources remain immutable even if a project artifact changes later.
React component libraries can produce one offline HTML file because all pinned
runtimes are embedded, while generated images remain explicit sidecars.
Adding a new UI library changes only catalog assets and metadata unless it
requires a genuinely new renderer capability.
