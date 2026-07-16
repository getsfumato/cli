# ADR-0003: Resolve Strict Templates By Stable Prompt ID

**Decision status:** Accepted and implemented for v0.2.

## Context

Drafting, title repair, structural validation repair, Mermaid repair, review,
layout repair, editing, compact recovery, and tool exhaustion need distinct
system and user messages. Inline strings duplicate rules and make provenance,
required variables, customization, and validation difficult to test.

## Decision

Define stable `PromptId` values for every system, user, compact, repair, edit,
and tool-exhausted template. A versioned manifest maps each ID to a bundled
`.md.j2` path and its required variables. A `PromptPair` selects the system and
user IDs for one provider request.

`PromptCatalog`, owned by `sfumato-core`, accepts a `PromptRenderRequest` with
structured `PromptVariables`. The MiniJinja implementation in
`sfumato-adapters` resolves a manifest-relative template path in this order:

1. `<project-root>/.sfumato/prompts/`;
2. the platform config directory's `sfumato/prompts/`;
3. bundled prompt assets.

The highest existing template fully overrides the lower source for that ID.
MiniJinja uses strict undefined values and no auto-escaping. Override paths and
includes must remain relative and contained; symlinks in override trees are
rejected. Rendering returns text plus prompt ID, source origin, manifest version,
and SHA-256 source hash.

Prompt customization may replace a system message. Structural safety therefore
does not rely only on prompt text: output parsing, revision tests, patch
allowlists, path containment, tool permissions, and artifact validation remain
code-owned. Source material and rejected responses are delimited as data by the
bundled templates. Root `SFUMATO.md` remains a typed template variable.

## Consequences

- Every ID has manifest/enum parity and golden rendering tests.
- A user can customize one message without forking the pipeline.
- Generation output and committed artifact manifests report exact template
  provenance.
- Invalid or stale overrides fail before a model request instead of silently
  falling back.
- Template authors must track the manifest's required variables.

## Authoring Contract

The stable IDs, locations, variables, precedence, and MiniJinja rules are
normative in [Prompt Authoring](../reference/prompt-authoring.md).
