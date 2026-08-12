# Configuration And Projects

## Persisted Documents

Sfumato uses three schema-v5 TOML documents:

| Scope | Platform-neutral location | Content |
| --- | --- | --- |
| Global | `<platform-config>/sfumato/config.toml` | User profile, connectors, model profiles, user model defaults, reviewer role, and global Marp settings. |
| Registry | `<platform-config>/sfumato/projects.toml` | Registered project names, paths, and the active project. |
| Project | `<project-root>/.sfumato/project.toml` | Theme, publication directory, project model defaults, reviewer, page defaults, tools, security, and optional Marp override. |

On macOS, `<platform-config>` normally resolves under
`~/Library/Application Support`. Linux normally uses `$XDG_CONFIG_HOME` or
`~/.config`. Use `sfumato config show` rather than assuming a platform path.

Managed generated artifacts do not live in the project source root:

```text
~/.sfumato/Projects/<project>/resources/<resource-kind>/<resource-id>/
```

The project root contains only portable project configuration, optional prompt
overrides, project artifacts, and optional `SFUMATO.md` guidance.

## Schema Version And Migration

Every persisted document has `schema_version = 5`.

- Project and global schema v4 documents are migrated once to v5.
- Migration writes an adjacent `.v4.bak` backup before atomic replacement.
- Legacy page plugin entries are classified as one UI library, utility plugins,
  or internal runtime dependencies.
- If multiple legacy UI libraries exist, the last wins and a migration comment
  records the replacement.
- Missing versions, versions 1 through 3, and future versions are rejected
  without rewriting.
- Reads of an existing schema-v5 document are side-effect free.
- Writes validate the entire resulting document, take a lock, and replace it
  atomically.

## Effective Resolution

Configuration is resolved from lowest to highest precedence:

1. global user configuration;
2. selected project configuration;
3. command-line overrides.

Model capability resolution is specifically:

1. `--model capability=profile` on the generation command;
2. project `[model_defaults]`;
3. global `[defaults]`.

Reviewer resolution is:

1. `--review-model <profile>`;
2. project `[model_roles].reviewer`;
3. global `[model_roles].reviewer`;
4. the resolved text drafter profile.

Theme resolution is:

1. generation `--theme <name>`;
2. project `theme`.

Generation tool resolution is:

1. command `--tool` or `--disable-tool`;
2. project `[generation_tools]`;
3. internal default: `image-gen` is enabled when an image default exists and
   `video-gen` is disabled.

## Project Configuration Example

```toml
schema_version = 5
name = "university"
theme = "gruvbox"
publish_dir = "Resources"

[model_defaults]
text = "codex"
code = "codex"
image = "gpt-image"
video = "openrouter-video"

[model_roles]
reviewer = "grok-latest"

[page]
ui = "shadcn"
plugins = ["motion"]

[generation_tools]
image_gen = true
video_gen = false
audio_gen = true

[security]
allow_python = false

[knowledge]
backend = "filesystem"

[marp]
pdf = true
```

Relative `publish_dir` values resolve from the registered project root. Absolute
values remain absolute. Managed revisions always remain under `~/.sfumato`.

## `init` Commands

### `sfumato init user [--yes] [--force]`

- Creates global schema-v5 configuration.
- Installs `sfumato-default` when it is absent.
- Interactive by default.
- `--yes` accepts non-interactive defaults.
- `--force` permits replacement of existing global configuration.

### `sfumato init project <name> [--path <path>] [--no-activate]`

- `name` must be a valid portable project name and must not duplicate a registry entry.
- `--path` defaults to the current directory.
- Creates `.sfumato/project.toml` at the resolved root.
- Registers the project globally.
- Activates it unless `--no-activate` is supplied.

## `project` Commands

### `sfumato project list`

Lists every registered project, its root, and which project is active.

### `sfumato project show [name]`

Shows the named project. When `name` is omitted, resolves the active project.
It reports the portable project settings. Use `project list` to inspect the
registered root path and active marker.

### `sfumato project use <name>`

Makes an existing registered project active. It does not change the current
shell directory.

### `sfumato project remove <name>`

Removes only the registry entry. It does not delete the project root,
`.sfumato/project.toml`, artifacts, or managed generated resources.

## `config` Commands

### Show

```bash
sfumato config show [key] [--scope user|project|effective] [--project <name>]
```

- Scope defaults to `effective`.
- `key` is an optional dotted path such as `defaults.text`,
  `model_defaults.image`, `page.ui`, or `generation_tools.image_gen`.
- `--project` selects a project when reading project or effective scope.
- Effective output is merged and read-only.
- Credential values are references and sensitive headers are redacted.

Examples:

```bash
sfumato config show
sfumato config show defaults.text --scope user
sfumato config show page.ui --scope project --project university
sfumato config show --scope effective --project university
```

### Set

```bash
sfumato config set <key> <value> [--scope user|project] [--project <name>]
```

- Scope defaults to `user`.
- The value is parsed as TOML when possible; otherwise it becomes a string.
- The complete document is validated before writing.
- Effective scope cannot be edited.
- Raw secret values must not be set through this command.

Examples:

```bash
sfumato config set user.name '"Alex"'
sfumato config set defaults.text codex
sfumato config set publish_dir '"Resources/Sfumato"' --scope project
sfumato config set generation_tools.image_gen true --scope project --project university
sfumato config set security.allow_python true --scope project --project university
```

Quote strings as TOML when shell ambiguity matters. A bare value that is not
valid TOML is stored as a plain string.

### Delete

```bash
sfumato config delete <key> [--scope user|project] [--project <name>]
```

Deletes one dotted key and validates the resulting document. Required fields
cannot be deleted because validation will reject the write.

```bash
sfumato config delete publish_dir --scope project --project university
sfumato config delete model_roles.reviewer --scope project --project university
```

## Choosing A Knowledge Source

A project grounds its resources in one of two places, set by
`knowledge.backend` in `.sfumato/project.toml`.

**`filesystem`** is the default and what every project did before the setting
existed. You pass source files or directories to a `generate` command, Sfumato
indexes them into the prompt, and the model reads what it judges relevant with
`sfumato_list_directory` and `sfumato_read_file`.

**`vitruvio`** grounds the project in a knowledge brain instead. The brain is a
local Vitruvio directory that returns evidence with its provenance — never prose
— and the model reaches it through a single tool, `sfumato_search_brain`:

```toml
[knowledge]
backend = "vitruvio"
project = "facultad"
brain = "algebra"
```

Two names, because Vitruvio addresses a brain in two steps: which project, then
which brain within it. `knowledge.project` is a project `vitruvio project
register` has made addressable by name, and Sfumato states both on every query
so that what a run reads is decided by this file rather than by the directory
the command was typed in.

For a Vitruvio project nobody registered, point at its file instead — the two
keys are alternatives, and naming both is refused:

```toml
[knowledge]
backend = "vitruvio"
config = "../vitruvio/vitruvio.toml"
brain = "algebra"
```

The brain must be reachable through the `vitruvio` command; set
`knowledge.executable` when it is not on `PATH`. Sfumato queries it as an agent
actor, so the brain records that a model asked.

What changes under a brain is deliberately small. The model is offered the
search tool instead of the two file tools, and the prompt carries an inventory of
the brain — which memory modules exist, how many blocks each holds, which filters
narrow anything — instead of a listing of files. Drafting, validation, diagrams,
layout, review, rendering, and publishing are untouched.

Two consequences are worth knowing before you switch:

- **Source paths are refused, not ignored.** `sfumato generate slides notes/` on
  a brain-backed project is an error naming the fix. Ignoring the paths would
  leave you believing a file shaped the deck when nothing did.
- **The model must interrogate, not query once.** The prompt pushes it to ask
  several distinct questions from different angles and to vary `memory_types`
  rather than only rewording. A run that made one search and stopped is a run
  that answered the question it already knew how to ask.

`SFUMATO.md` is still read under either backend: it is project guidance, not
source material.

## Project `SFUMATO.md`

Place optional model guidance at exactly:

```text
<project-root>/SFUMATO.md
```

Generation loads this UTF-8 file before sources and injects it separately into
drafting, semantic review, focused repair, and model-backed tool prompts.

Constraints:

- only the exact project-root file is read;
- maximum size is 64 KiB;
- it is not treated as ordinary source evidence;
- dry-run and structured generation results report whether it was loaded;
- instructions should describe durable project conventions, not one-off requests.

Example:

```markdown
# Sfumato project guidance

- Teach in Spanish.
- Prefer visual intuition before formulas.
- Define every symbol before using it.
- Use the course's engineering sign convention.
- Reuse registered project artifacts when semantically relevant.
```

## Portable Project Data

Project-owned reusable files live below `<project-root>/.sfumato/`:

```text
.sfumato/
├── project.toml
├── prompts/             # project prompt overrides
└── assets/              # reusable project artifact catalog and variants
```

Removing the global project registration preserves these files, allowing the
project to be registered again later.
