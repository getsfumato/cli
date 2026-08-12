---
name: sfumato-cli
description: The sfumato command surface — every group, what each command does, and which flag decides the outcome. Use when you need to find the right sfumato command, when a command failed and the flag may be wrong, when scripting sfumato, or when asked what sfumato can do.
allowed-tools: Bash(sfumato:*), Read
---

# The sfumato command surface

Fifteen groups. Four of them generate or change a resource; the other eleven
configure what a generation resolves. Every command takes the global
`--timeout <SECONDS>`; the exhaustive list of arguments and options per command
is in [cli-reference.md](../sfumato/references/cli-reference.md), generated from
the parser.

Running `sfumato` with no subcommand opens the terminal interface, and only when
stdin and stdout are a terminal. Scripts and agents pass a subcommand.

## Reading this

Each table names the flag that **decides the outcome** rather than every flag.
When two commands look interchangeable, the third column says which one you
actually want.

## `generate` — make a resource

| Command | Does | The flag that decides |
|---|---|---|
| `generate slides [INPUTS]...` | A Marp deck and its PDF. | `--instruction` (required). `--no-pdf` when Marp is missing or you only want Markdown. |
| `generate document [INPUTS]...` | Prose paginated to a PDF. Aliases `doc`, `docs`. | `--page-size`, `--toc`/`--no-toc`, `--cover`/`--no-cover`. |
| `generate page [INPUTS]...` | An HTML page that fetches nothing from the network. Alias `pages`. | `--ui` picks the single UI library; `--plugin` adds utilities. |
| `generate video [INPUTS]...` | An MP4. | `--engine` (required, no fallback) and `--duration` (required). |

Shared by all four: `--instruction`, `--title`, `--out`, `--project`, `--theme`,
`--model cap=profile`, `--review-model`, `--no-review`, `--tool`/
`--disable-tool`, `--brain`/`--brain-project`, `--allow-code-execution`,
`--dry-run`, `--json`.

`--template` is on slides, document, and page but **not** video — a film's
structure is its plan, not a marker in a file. `--no-pdf` is slides-only and
hidden from `--help`, though fully supported: it is the only way to turn a PDF
off for one run, since `marp.pdf = true` ships as the default and configuration
alone could only turn it on.

`--dry-run` resolves everything and renders the prompt without calling a
provider. Reach for it before any first run against a new project.

Depth on all four: [`sfumato-generate`](../sfumato-generate/SKILL.md).

## `edit` — change a resource without regenerating it

| Command | Does | The flag that decides |
|---|---|---|
| `edit slides <DECK>` | Applies one focused instruction to a generated deck as a JSON Patch, and commits a **new** revision. | `--instruction` (required). `--model text=...` is the only meaningful override. |

The deck must be a Sfumato-generated Marp `.md` inside the selected project's
managed artifact root. An arbitrary vault file is rejected. Read
`markdown_path`/`pdf_path` from the result — the edit lives somewhere new.

## `video` — the paused review checkpoint

| Command | Does | The flag that decides |
|---|---|---|
| `video preview <REVIEW_ID>` | Prints where the source bundle, snapshots, and contact sheet are. | — |
| `video approve <REVIEW_ID>` | Renders and publishes the paused film. | `--out` only when overriding the destination saved at generation. |

Reached by generating with `--visual-review`, which stops before the encode.

## `init` — first run

| Command | Does | The flag that decides |
|---|---|---|
| `init user` | Global config, bundled theme, first connector and model. | `--yes` skips the questions; `--force` replaces an existing config. |
| `init project <NAME>` | Registers a directory as a project and writes `.sfumato/project.toml`. | `--path` (default `.`), `--no-activate` to register without switching to it. |

## `project` — which body of work

| Command | Does |
|---|---|
| `project list` | Every registered project and which is active. |
| `project show [NAME]` | One project's root and settings. |
| `project use <NAME>` | Change the active project. |
| `project remove <NAME>` | Unregister. Does not delete the directory. |

Agents should pass `--project` per command rather than depend on the active one:
another terminal can change it underneath a running script.

## `config` — the three layers

| Command | Does | The flag that decides |
|---|---|---|
| `config show [KEY]` | Prints configuration, optionally one dotted key. | `--scope user\|project\|effective` (default `effective`). |
| `config set <KEY> <VALUE>` | Writes one dotted key. | `--scope` (default **`user`**, unlike `show`). |
| `config delete <KEY>` | Removes one dotted key. | `--scope` (default `user`). |

Two traps worth stating. `show` defaults to `effective` while `set` defaults to
`user`, so reading a key and writing it back without `--scope` can move it
between layers. And `set` validates the whole file after writing, so an
intermediate state that is invalid on its own is refused — order your writes so
the file is valid after each one.

## `connector` — how to reach a provider

| Command | Does | The flag that decides |
|---|---|---|
| `connector presets` | The six presets `setup` accepts. | — |
| `connector setup <PRESET>` | Configures one. | `--name` for several off one preset; `--api-key-env` to reference an environment variable instead of the keyring. |
| `connector list` / `show <NAME>` | Configured connectors, redacted. | — |
| `connector capabilities <NAME>` | Which generation and native features it exposes. | — |
| `connector models <NAME>` | The provider's own catalog. Discovery, not profile creation. | — |
| `connector status <NAME>` | Native runtime or account state. | — |
| `connector login` / `auth-status` / `logout <NAME>` | Credential in the OS keyring. | — |

Presets: `ollama`, `lmstudio`, `openrouter`, `anthropic`, `codex`, `elevenlabs`.
Never run `connector login codex` — Codex owns its own authentication through
`codex login`.

## `model` — which model does what

| Command | Does | The flag that decides |
|---|---|---|
| `model add <NAME>` | A profile binding a connector, a provider model ID, and capabilities. | `--connector`, `--id`, `--capability` (all required); `--option key=value`. |
| `model edit <NAME>` | Changes one. | Any `--capability` **replaces the whole set**; `--option` merges by key. |
| `model list` / `show <NAME>` | What is configured. | — |
| `model remove <NAME>` | Deletes a profile. Refused while a default or role points at it. | — |
| `model use <SELECTOR> <PROFILE>` | Assigns a default. | `--project` writes the project layer; omitting it writes the **user** layer. |

`<SELECTOR>` is a capability — `text`, `code`, `image`, `video`, `speech`,
`embedding` — or the `reviewer` role.

## `theme` — one per project

| Command | Does | The flag that decides |
|---|---|---|
| `theme create <NAME>` | Copies the bundled scaffold. | — |
| `theme import <PATH>` | Builds a theme from a Google DESIGN.md. | `--name` overrides the derived name. |
| `theme export <NAME>` | Writes a self-contained DESIGN.md. | `--out` (default `DESIGN.md`). |
| `theme list` / `show <NAME>` | Installed themes and one manifest. | — |
| `theme use <NAME>` | Sets the project's theme. | `--project`. |
| `theme regenerate [NAME]` | Re-derives renderer stylesheets from the manifest. | Omit the name to do every installed theme. |

Reach for `regenerate` after hand-editing a `theme.toml`, and after a Sfumato
upgrade that changed how a stylesheet is derived.

## `template` — optional structure, never implicit

| Command | Does | The flag that decides |
|---|---|---|
| `template create <NAME>` | Scaffolds a package. | `--kind slides\|page\|document` (required); `--from <PATH>` to adopt existing markup. |
| `template list` | Installed templates. | `--kind` filters. |
| `template show <NAME>` | Metadata and the full structural source. | `--kind` optional — the package declares its own. |

A template is used only when a generation passes `--template <name>`.

## `artifact` — reusable project visuals

| Command | Does | The flag that decides |
|---|---|---|
| `artifact add <PATH>` | Copies and registers a logo, chart, icon, or diagram. | `--theme` for one theme, `--all-themes` for the wildcard; they conflict. `--prompt` records how to regenerate it for another theme. |
| `artifact edit <NAME>` | Metadata or a variant reassignment. | `--from-theme` + `--to-theme` together; `--tag` **replaces** all tags. |
| `artifact list` / `show <NAME>` | The catalog and one entry. | `--project`. |
| `artifact remove <NAME>` | Drops the entry and managed copies. Never touches the original file. | `--project`. |

## `prompt` — the model-facing language

| Command | Does | The flag that decides |
|---|---|---|
| `prompt list` | Every valid prompt ID. The authoritative list. | `--project`. |
| `prompt show <ID>` | The resolved source, its origin, version, and hash. | `--project`. |
| `prompt customize <ID>` | Copies the bundled source into an editable override. Opens no editor. | `--scope user\|project` (required). |
| `prompt validate` | Renders every resolved template against fixtures. | `--project`. |

Run `prompt validate` after editing an override. An invalid override stops a
generation before the provider is called — Sfumato never silently falls back
past one.

## `plugin` — offline page libraries

| Command | Does | The flag that decides |
|---|---|---|
| `plugin list` | Catalog version, installed version, project enablement. | `--project`. |
| `plugin show <ID>` | Category, API global, model guidance, hash, licence. | — |
| `plugin install <ID>` | The only command allowed to download. | `--version`. |
| `plugin update <ID>` | Installs the catalog's current pin. | — |
| `plugin enable` / `disable <ID>` | Project page defaults. | `--project`. |

A UI plugin replaces the current UI; utilities accumulate. Generation never
downloads anything.

## `tool` — what the model may call

| Command | Does | The flag that decides |
|---|---|---|
| `tool list` | Whether each tool is enabled and whether a profile backs it. | `--project`. |
| `tool enable` / `disable <TOOL>` | Project default for one tool. | `--project`. |

Tools: `image-gen`, `video-gen`, `audio-gen`, `chart-gen`. Per-run overrides are
`--tool` and `--disable-tool` on a generation.

## `renderer` — the local machinery

| Command | Does | The flag that decides |
|---|---|---|
| `renderer list` | Pinned version, installation, health, dependencies. | — |
| `renderer install <RENDERER>` | The only command allowed to run npm/uv and download. | — |
| `renderer remove <RENDERER>` | Deletes the managed installation, not generated output. | — |
| `renderer doctor [RENDERER]` | Checks one, or all when omitted. | — |

Renderers: `hyperframe`, `manim`, `pagedjs`. Sfumato never falls back between
them; a missing renderer is an error naming what to install.

## Finding your way

- "What can this do?" → this file, then
  [cli-reference.md](../sfumato/references/cli-reference.md).
- "Which command creates X?" → `generate`, above.
- "It failed and I think a flag is wrong" → the third column here, then
  [error-contract.md](../sfumato/references/error-contract.md) for `error.stage`.
- "What is configured right now?" → `config show --scope effective --project X`,
  plus `model list`, `tool list --project X`, `renderer doctor`.
