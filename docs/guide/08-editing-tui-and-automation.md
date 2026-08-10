# Editing, TUI, And Automation

This chapter covers focused editing of an existing generated deck, the
interactive terminal interface, and the conventions an automation agent should
follow. Generation itself is documented in the resource-specific chapters.

## Focused Slide Editing

Use `edit slides` when a generated Marp deck is structurally correct but its
content needs a targeted change:

```text
sfumato edit slides <DECK> \
  --instruction <TEXT> \
  [--project <NAME>] \
  [--model text=<PROFILE>]... \
  [--json]
```

Example:

```bash
sfumato edit slides \
  "$HOME/.sfumato/Projects/university/resources/slides/fourier-series/revisions/<revision>/deck.md" \
  --project university \
  --instruction "Correct the phase-shift example and add one concise worked step"
```

### Arguments And Flags

| Input | Required | Meaning |
| --- | --- | --- |
| `<DECK>` | Yes | Path to a generated Marp `.md` file in the selected project's managed artifact store. |
| `--instruction <TEXT>` | Yes | A non-empty description of the specific content change. |
| `--project <NAME>` | No | Selects a registered project; otherwise the active project is used. |
| `--model text=<PROFILE>` | No | Overrides the text profile used to propose the edit. Other capabilities are rejected. The flag is repeatable syntactically, but only a text mapping is meaningful. |
| `--json` | No | Prints the result or a structured error to stdout and suppresses interactive progress. |

The deck must satisfy all of these conditions:

- It has the `.md` extension.
- Its canonical path is inside the selected project's managed artifact root.
- It is a generated Marp deck with a recognizable title slide and theme.
- Its theme still exists in the theme repository.

An arbitrary Markdown file in the vault is deliberately rejected. Register and
generate the deck through Sfumato first, and select the matching project with
`--project`.

### Editing Contract

Sfumato does not ask the model to regenerate the whole deck. It parses the deck
into a `SlideDeckDocument`, exposes a revisioned snapshot, and asks for an RFC
6902 JSON Patch. The patch contract permits focused slide-content changes while
protecting the document schema and revision guard.

The workflow is:

1. Canonicalize the input path and verify project ownership.
2. Parse frontmatter, title, slide boundaries, fenced blocks, images, and
   Mermaid content.
3. Load project `SFUMATO.md`, the deck's theme, and the resolved text model.
4. Request a constrained JSON Patch for the instruction.
5. Apply the patch transactionally to the reviewable document.
6. Normalize and validate the complete candidate deck.
7. Render Mermaid diagrams and inspect the real Marp layout.
8. Run focused repair when the supported repair path can improve an issue.
9. Render a fresh PDF with the deck theme.
10. Commit a new immutable artifact revision linked to its parent.

The original revision remains unchanged. Consume the `markdown_path` and
`pdf_path` returned by the command because the successful edit lives in a new
revision directory.

### Human Output

On success, the command reports the patch operation count, changed slide IDs,
selected model, new Markdown path, and new PDF path. If the patch contains no
content changes, Sfumato still regenerates the PDF and reports that no content
changes were needed. Non-fatal warnings are written to stderr.

### JSON Result

With `--json`, stdout contains an `EditSlidesResult` object:

```json
{
  "project": "university",
  "model": "codex",
  "markdown_path": "/.../revisions/<new-revision>/deck.md",
  "pdf_path": "/.../revisions/<new-revision>/deck.pdf",
  "project_instructions": "/.../SFUMATO.md",
  "operations": 2,
  "changed_slides": ["phase-shift-example"],
  "context_compacted": false,
  "layout_issues": [],
  "artifacts": ["/.../deck.md", "/.../deck.pdf"],
  "warnings": [],
  "prompts": []
}
```

Paths and prompt provenance vary by run. Treat the returned fields as the
authority rather than constructing revision paths manually.

## Interactive Terminal Interface

Run Sfumato without a subcommand:

```bash
sfumato
```

The TUI opens only when stdin and stdout are attached to an interactive
terminal. For scripts, CI, pipes, and agents, use explicit commands instead.

### Main Sections

Thirteen destinations in three groups. The grouping is the point: creating
something is a different kind of act from maintaining the library it draws on.

**CREATE**

| Section | Available work |
| --- | --- |
| Generate | Slides, document, page, or video, with resource-specific fields. |
| Edit | Apply a focused instruction to an existing generated deck. |

**LIBRARY**

| Section | Available work |
| --- | --- |
| Projects | Create, activate, edit, and remove project registrations. |
| Models | Add, edit, select defaults, and remove model profiles. |
| Connectors | Configure any of the six presets; inspect native model catalogs and status. |
| Themes | Create, import, export, apply, and regenerate themes. |
| Templates | Create reusable slide, page, or document structures. |
| Artifacts | Add, edit, or remove reusable project visuals. |
| Prompts | Create user/project overrides and validate prompt templates. |

**SETTINGS**

| Section | Available work |
| --- | --- |
| Tools | Switch `image-gen`, `video-gen`, `audio-gen`, and `chart-gen` on or off, per project. |
| Plugins | Install, enable, and disable offline page libraries. |
| Configuration | Set or delete validated dotted configuration values. |
| Setup | Initialize the user profile or create a project. |

Tools and Plugins exist here because `sfumato tool` and `sfumato plugin` had no
TUI entry at all, which made "enable chart-gen" unreachable from the interface
that offers everything else. A test now asserts every nav entry dispatches
somewhere.

The browse screens expose actions for the selected section. They intentionally
do not expose every advanced CLI flag; use the explicit command reference for
complete control.

### Resource-Specific Generation Forms

Changing the resource selector rebuilds the form with only relevant fields:

- Slides expose the slide template, PDF publication directory, and image tool.
- Pages expose the page template, exclusive UI library, utility plugins,
  publication directory, image tool, and video tool.
- Videos expose MP4 publication, engine, duration, resolution, aspect ratio,
  audio, local FPS/quality, code authorization, and image tool as applicable.

Changing the video engine similarly removes incompatible options. Each resource
keeps its own in-progress form values while switching between resource types.

### Pickers Instead Of Identifiers

Fields that name something you already have — a project, a theme, a template, a
model profile — open a filterable picker rather than taking free text. Type to
filter, `Up`/`Down` to move, `Enter` to choose, `Del` to clear the field, `Esc` to
cancel. Capability-filtered where filtering is what makes the list correct: a
slides template is not offered to a document, and an image profile is not offered
where a text one is wanted, because the layers underneath reject both.

### Key Bindings

The footer shows the keys for the current screen, and it is generated per screen,
so it is the authoritative list. What follows is the shape of it.

Global, from every screen including one with a form or picker open:

| Key | Action |
| --- | --- |
| `Ctrl+Q`, `Ctrl+C` | Open the quit confirmation. |
| `Ctrl+K` | Open the jump palette and go straight to any destination. |
| `?` | Open the key reference. Not on Generate or Edit, where `?` is text. |

Leaving is a deliberate gesture: the confirmation requires `y`, and `Enter` is
deliberately not a confirmation. A generation that took minutes and a stray
keystroke should not be able to meet.

Home:

| Key | Action |
| --- | --- |
| `Up`, `k` | Select the previous section. |
| `Down`, `j` | Select the next section. |
| `Enter` | Open the selected section. |
| `Esc` | Clear the status message. |

`q` does nothing on Home and `Esc` no longer quits from it; ending a session goes
through the confirmation above.

### While A Run Is Going

The run screen is a progress line and an activity feed rather than a spinner: the
feed names each tool call, each review finding, and each repair as it happens, and
previews generated images inline where the terminal supports it. When the run
finishes it says where the resource landed, and offers to publish it beside the
sources it was made from.

Browse screens:

| Key | Action |
| --- | --- |
| `Up`, `k` / `Down`, `j` | Select a row. |
| `Left`, `h` / `Right`, `l` | Select an available action. |
| `Tab`, `BackTab` | Move focus between rows and actions. |
| `Enter` | Move from a row to actions, or execute the selected action. |
| `PageUp`, `PageDown` | Scroll the selected row's details. |
| `r` | Reload the current section; after a connector native query it returns to the configured connector list. |
| `Esc`, `Backspace` | Return home. |

Operation dialogs:

| Key | Action |
| --- | --- |
| `Up`, `Down`, `Tab`, `BackTab` | Move between fields. |
| Typing / `Backspace` | Edit the selected text field. |
| `Space`, `Enter` | Toggle a boolean field. |
| `Enter` on submit | Execute the operation. |
| `Esc` | Close the dialog without applying it. |

Generate and edit forms:

| Key | Action |
| --- | --- |
| `Up`, `Down`, `Tab`, `BackTab` | Move between fields. |
| `Left`, `Right` | Change a select value or move the multiselect cursor. |
| `Space` | Toggle a boolean or the current multiselect item. |
| Typing / `Backspace` | Edit the current text field. |
| `Shift+Enter` | Insert a newline in a multiline instruction field. |
| `Enter` | Advance, toggle/select, or submit according to the field type. |
| `Esc` | Return home. |

Running and completion screens:

| Key | Action |
| --- | --- |
| `Up`, `k` / `Down`, `j` | Browse model, tool, warning, and result activities; generated images may be previewed. |
| `Esc` while running | Request cancellation. No staged artifact is committed after a successful cancellation checkpoint. |
| `Esc`, `Backspace` after completion | Return home. |
| `Enter` after completion | Return to the generate or edit form that launched the job. |

## Automation And Agent Integration

### Prefer Explicit Project Selection

Interactive users may rely on the active project. Agents should normally pass
`--project <NAME>` so another terminal changing the active project cannot alter
the operation unexpectedly.

### Use Dry Run For Resolution

`--dry-run` is available on generation commands. It resolves the project,
theme, models, tools, plugins or renderer requirements, source bundle,
`SFUMATO.md`, artifacts, and prompt templates without model calls or artifact
writes. It is suitable for preflight checks, but it does not prove that a
provider, browser, or renderer will succeed at runtime.

Do not combine human dry-run text parsing with production automation. Use it for
diagnostics, then execute the real command with `--json`.

### Machine-Readable Output

Generation and slide editing support `--json`:

```bash
sfumato generate page \
  --project university \
  --instruction "Explain convolution interactively" \
  --json > result.json
```

On success, stdout is one pretty-printed JSON object. Progress events are not
printed. A typed failure also writes one JSON object to stdout and exits
non-zero:

```json
{
  "error": {
    "code": "provider",
    "class": "retry",
    "retryable": true,
    "stage": "draft",
    "message": "sanitized provider message",
    "details": {}
  }
}
```

Possible `code` values are `config`, `validation`, `not_found`, `provider`,
`tool`, `render`, `artifact`, `cancelled`, and `internal`. Possible `class`
values are `retry`, `context_limit`, `invalid_output`, `unavailable`,
`cancelled`, and `permanent`.

Validation performed by the presentation layer before entering a typed core use
case may return the smaller fallback shape below. Agents must support both:

```json
{
  "error": {
    "message": "Instruction cannot be empty"
  }
}
```

An agent should:

1. Check the process exit status.
2. Parse stdout as JSON.
3. If `error.retryable` is absent or false, change configuration or input before retrying.
4. If it is true, use `class` and `stage` to choose bounded retry, dependency
   recovery, context reduction, or corrected generated output.
5. Read `artifacts`, `published_artifacts`, `html_path`, `video_path`,
   `markdown_path`, or `pdf_path` from the response.
6. Preserve `prompts` and model selections when provenance matters.

Warnings are part of page/video JSON result objects and slide/edit result data.
Human mode may also write warnings to stderr. Never infer success from a path
appearing in logs; require exit status zero and a parsed result.

### Shell Quoting

Quote paths and instructions that contain spaces, apostrophes, shell symbols,
or non-ASCII characters:

```bash
sfumato generate slides \
  "/Users/alex/Documents/Alex's Notebook/Facultad/notes" \
  --instruction "Explicá la transformada de Laplace paso a paso" \
  --project university \
  --json
```

Do not interpolate untrusted text into a shell command string. Process-spawning
agents should pass each argument separately.

### Stable Agent Sequence

For a fully deterministic invocation:

```bash
sfumato project show university
sfumato config show --scope effective --project university
sfumato connector auth-status openrouter
sfumato model show codex
sfumato prompt validate --project university
sfumato generate slides --project university --instruction "..." --dry-run
sfumato generate slides --project university --instruction "..." --json
```

Add `plugin list --project`, `tool list --project`, or `renderer doctor` when
the selected resource requires those subsystems.

### Cancellation And Atomicity

The TUI owns cancellable job handles. Explicit CLI commands can also receive
normal process termination signals from their caller. Workflows check
cancellation/deadlines at stage boundaries and external operations.

Managed resources are staged first and committed only after validation. A
failed or cancelled transaction must not become the current revision.
Publication happens after the managed commit; therefore a publication failure
can leave a valid managed revision and return a warning or publication error.
