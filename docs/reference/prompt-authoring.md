# Prompt Authoring

**Implementation status:** Active in v0.2.

Sfumato templates are UTF-8 Markdown with MiniJinja expressions. A template
fully defines one model-facing system or user message. Code still enforces patch,
path, tool, document, and artifact invariants after the model responds.

## Resolution Roots

For the relative path named by the bundled manifest, the first existing source
wins:

1. `<project-root>/.sfumato/prompts/<relative-path>`;
2. `~/.config/sfumato/prompts/<relative-path>`;
3. the bundled template.

For example, a project override for the draft user message is:

```text
<project-root>/.sfumato/prompts/slides/draft.user.md.j2
```

Overrides keep the exact bundled relative path. Unknown `.j2` files may support
contained `{% include %}` statements but are not directly renderable by ID.
Absolute paths, `..`, symlinks, and includes that escape an override root are
rejected. Each template is limited to 64 KiB. Invalid UTF-8 or an oversize file
is an error, not a fallback.

## Stable Prompt IDs

| Workflow | System ID | User ID |
| --- | --- | --- |
| Draft | `slides.draft.system` | `slides.draft.user` |
| Compact draft | `slides.compact-draft.system` | `slides.compact-draft.user` |
| Title repair | `slides.title-repair.system` | `slides.title-repair.user` |
| Review | `slides.review.system` | `slides.review.user` |
| Compact review | `slides.compact-review.system` | `slides.compact-review.user` |
| Layout repair | `slides.layout-repair.system` | `slides.layout-repair.user` |
| Compact layout repair | `slides.compact-layout-repair.system` | `slides.compact-layout-repair.user` |
| Edit | `slides.edit.system` | `slides.edit.user` |
| Compact edit | `slides.compact-edit.system` | `slides.compact-edit.user` |

Image generation uses the user template `image.generation.user`. The image tool
renders it with the requested visual, selected theme tokens, and project
instructions; its provenance is included in the committed revision manifest.

Tool exhaustion uses user-message IDs
`slides.draft.tool-exhausted.user`,
`slides.review.tool-exhausted.user`,
`slides.edit.tool-exhausted.user`, and
`slides.layout-repair.tool-exhausted.user`.

The manifest and `PromptId::all()` must contain exactly the same IDs. Adding or
renaming an ID is a reviewed prompt-schema change.

## MiniJinja Contract

Templates use strict undefined behavior and no auto-escaping. A missing required
variable, unknown filter, syntax error, or invalid include fails before the model
request. Use normal MiniJinja syntax:

```jinja
You create study slides for a learner who prefers {{ learning_style | join(", ") }}.

Project: {{ project }}
Instruction: {{ instruction }}

{{ project_instructions }}
```

Do not render secrets. Variables are serialized structured values, not an
unrestricted application object.

## Variables

The manifest is authoritative. Common required values are:

| Templates | Required values |
| --- | --- |
| Draft | `learning_style`; user message also receives `project`, `project_root`, `theme_name`, `theme_colors`, `theme_fonts`, `instruction`, `project_instructions`, `title`, `image_generation_available`, `source_bundle`. |
| Title repair | `project`, `instruction`, `validation_error`, `project_instructions`, `headings`. |
| Review | `instruction`, `project`, theme values, `project_instructions`, retry values, source bundle, and `snapshot`. |
| Layout repair | `instruction`, `title`, `project`, `theme_name`, `project_instructions`, `issue_report`, retry values, and `slide_markdown`. |
| Edit | `project`, `instruction`, theme values, `project_instructions`, retry values, and `snapshot`. |
| Tool exhausted | `max_tool_rounds`. |

Compact variants use compact source/snapshot values selected by the use case.
Retry variables are always present and indicate whether corrective feedback is
active, avoiding optional-variable ambiguity.

## Structural Safety

Templates may change tone, pedagogy, and even model instructions, but cannot
change code-owned acceptance rules. In particular:

- review and edit patches must pass RFC 6902 parsing and revision tests;
- edit may replace existing slide Markdown only;
- title/frontmatter/ID/order protection is enforced after rendering;
- filesystem tools and artifact paths remain contained;
- incomplete or invalid model output never commits.

Authors should preserve clear delimiters around source documents, deck
snapshots, rejected responses, and tool results because those values are data.

## Provenance And Validation

Every render records prompt ID, `bundled`, `user`, or `project` origin, manifest
schema version, and SHA-256 hash of the selected template source. Dry-run and
result metadata may expose provenance but not source bundles or secrets.

The target prompt commands or equivalent facade methods list IDs, show the
selected source, copy a bundled template into user/project scope without
overwriting, and validate all templates. Template changes require manifest
parity, precedence, strict-variable, include-containment, and golden render tests.
