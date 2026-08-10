# Prompt Authoring

**Implementation status:** Active in v0.2.

Sfumato templates are UTF-8 Markdown with MiniJinja expressions. A template
fully defines one model-facing system or user message. Code still enforces patch,
path, tool, document, and artifact invariants after the model responds.

## Resolution Roots

For the relative path named by the bundled manifest, the first existing source
wins:

1. `<project-root>/.sfumato/prompts/<relative-path>`;
2. `<platform-config>/sfumato/prompts/<relative-path>`;
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

`<platform-config>` is the operating-system directory described in
[Configuration v5](config-v5.md), such as `~/Library/Application Support` on
macOS or `$XDG_CONFIG_HOME` on Linux.

## Stable Prompt IDs

| Workflow | System ID | User ID |
| --- | --- | --- |
| Draft | `slides.draft.system` | `slides.draft.user` |
| Compact draft | `slides.compact-draft.system` | `slides.compact-draft.user` |
| Title repair | `slides.title-repair.system` | `slides.title-repair.user` |
| Structural validation repair | `slides.validation-repair.system` | `slides.validation-repair.user` |
| Review | `slides.review.system` | `slides.review.user` |
| Compact review | `slides.compact-review.system` | `slides.compact-review.user` |
| Mermaid repair | `slides.mermaid-repair.system` | `slides.mermaid-repair.user` |
| Layout repair | `slides.layout-repair.system` | `slides.layout-repair.user` |
| Compact layout repair | `slides.compact-layout-repair.system` | `slides.compact-layout-repair.user` |
| Edit | `slides.edit.system` | `slides.edit.user` |
| Compact edit | `slides.compact-edit.system` | `slides.compact-edit.user` |
| Page draft | `page.draft.system` | `page.draft.user` |
| Compact page draft | `page.compact-draft.system` | `page.compact-draft.user` |
| Page validation repair | `page.validation-repair.system` | `page.validation-repair.user` |
| Page review | `page.review.system` | `page.review.user` |
| Page browser repair | `page.browser-repair.system` | `page.browser-repair.user` |
| Video plan | `video.plan.system` | `video.plan.user` |
| Video plan review | `video.review.system` | `video.review.user` |
| Hyperframes scene authoring | `video.hyperframe-scene.system` | `video.hyperframe-scene.user` |
| Manim authoring | `video.manim.system` | `video.manim.user` |
| Video source repair | `video.source-repair.system` | `video.source-repair.user` |

Image generation uses the user template `image.generation.user`. Page video
generation uses `video.generation.user`; it receives the requested asset,
accessible description, active theme tokens, project instructions, and reusable
artifact references before dispatching the remote video model. The image tool
renders it with the requested visual, selected theme tokens, and project
instructions; its provenance is included in the committed revision manifest.

Tool exhaustion uses user-message IDs
`slides.draft.tool-exhausted.user`,
`slides.review.tool-exhausted.user`,
`slides.edit.tool-exhausted.user`, and
`slides.layout-repair.tool-exhausted.user`.
Page generation uses `page.tool-exhausted.user`.

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

`source_bundle` is a pointer to the material, not the material. Under the default
filesystem grounding it is an index of the supplied files — a directory tree with
sizes; under a Vitruvio brain it is an inventory of the brain — its memory
modules, their block counts, and the columns that can be filtered on. Templates
that receive it also carry the reading tools and must tell the model to go and
get what it needs. `shared/source-index.md.j2` is the partial that does so, and
an override that drops it leaves the model guessing from filenames or from a
module list.

`knowledge_backend` selects which half of that partial renders: `"filesystem"`
or `"vitruvio"`. It is required by every prompt that includes the partial, so an
override must keep the branch or supply its own equivalent for both groundings.
The brain half additionally has to carry the interrogation discipline — ask
several distinct questions, vary `memory_types` — because the search tool is a
plain search and asking well is the model's job; a partial that drops those lines
leaves nothing enforcing them.

`compact` is set on the retry that runs without tools. That path carries content
rather than pointers: bounded file excerpts under a filesystem grounding, and the
evidence the model already retrieved under a brain, via
`shared/compact-sources.md.j2`.

A new tool's descriptions get their **own** prompt id rather than new keys on an
existing one. `ToolDescriptions` and `BrainToolDescriptions` both reject unknown
fields and are rendered from files a project may have copied and edited, so
adding a required key to a shared descriptions prompt breaks every project that
overrode it — including projects that will never see the tool.

| Templates | Required values |
| --- | --- |
| Draft | `learning_style`; user message also receives `project`, `project_root`, `theme_name`, `theme_colors`, `theme_fonts`, `instruction`, `project_instructions`, `title`, `image_generation_available`, `source_bundle`, `compact`, `knowledge_backend`. |
| Title repair | `project`, `instruction`, `validation_error`, `project_instructions`, `headings`. |
| Structural validation repair | `instruction`, `title`, `project`, theme values, `project_instructions`, `validation_error`, and `draft_markdown`. |
| Review | `instruction`, `project`, theme values, `project_instructions`, retry values, source bundle, and `deck_snapshot`. |
| Mermaid repair | `instruction`, `project`, theme values, `project_instructions`, `validation_error`, and `deck_snapshot`. |
| Layout repair | `instruction`, `title`, `project`, `theme_name`, `project_instructions`, `issue_report`, retry values, and `slide_markdown`. |
| Edit | `project`, `instruction`, theme values, `project_instructions`, retry values, and `deck_snapshot`. |
| Tool exhausted | `max_tool_rounds`. |
| Page draft | Project, theme, instruction, project guidance, sources, selected plugin guides, optional title, and image-generation availability. |
| Page validation repair | Instruction, theme, selected plugins, rejected draft response, and static validation error. |
| Page review | Instruction, project/theme context, project guidance, sources, and `page_snapshot`. |
| Page browser repair | Instruction, theme, selected plugins, `page_snapshot`, and structured desktop/mobile `issue_report`. |
| Video plan | Instruction, sources, project guidance, theme tokens, duration, engine, artifacts, and image-tool availability. |
| Video authoring | Reviewed production plan (workflow, DESIGN direction, storyboard beats, catalog allowlist) plus exact resolution, aspect, FPS, theme, and offline source contract. |
| Video planning | Workflow intent, URL references, sources, theme, managed catalog summary, and the strict v2 plan schema. |
| Video visual review | Reviewed plan, deterministic snapshot/contact-sheet paths, renderer diagnostics, and one focused repair budget. |
| Video source repair | Renderer engine, immutable source snapshot, and the static or process validation error. |

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
- page drafts remain structured JSON fragments; semantic review may patch the
  four content fields, while browser repair cannot patch the title;
- page templates cannot enable remote scripts, imports, network calls, path
  traversal, unregistered assets, or weaken the adapter-owned offline CSP.
- page mathematics uses `\(...\)` for inline TeX and `\[...\]` for display
  TeX; templates must not request CDN scripts or model-authored math renderers;
- generated page images must preserve the exact `assets/images/...` reference
  returned by `sfumato_image_gen`.

Authors should preserve clear delimiters around source documents, deck
snapshots, rejected responses, and tool results because those values are data.

## Provenance And Validation

Every render records prompt ID, `bundled`, `user`, or `project` origin, manifest
schema version, and SHA-256 hash of the selected template source. Generation
results and committed revision manifests retain that provenance without source
bundles or secrets.

The target prompt commands or equivalent facade methods list IDs, show the
selected source, copy a bundled template into user/project scope without
overwriting, and validate all templates. Template changes require manifest
parity, precedence, strict-variable, include-containment, and golden render tests.
