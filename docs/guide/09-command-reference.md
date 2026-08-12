# Complete Command Reference

This is the exhaustive command-line reference for the current Sfumato binary.
Run `sfumato <group> <command> --help` to confirm syntax in the installed build.
Conceptual behavior and end-to-end workflows are explained in the other guide
chapters; this file is optimized for exact command discovery by agents.

## Top Level

```text
sfumato [--timeout <SECONDS>] [COMMAND]
```

With no command, Sfumato opens the TUI when **both** stdin and stdout are
interactive terminals; otherwise it prints help and exits 0, so piping into
`sfumato` never opens the interface.

The fifteen command groups are `init`, `config`, `project`, `connector`, `model`,
`theme`, `template`, `artifact`, `prompt`, `plugin`, `tool`, `renderer`, `video`,
`generate`, and `edit`.

| Global flag | Effect |
| --- | --- |
| `--timeout <SECONDS>` | Abandon the operation after this many seconds; unbounded when omitted. Global because it bounds anything that waits outside the process — a provider request, a renderer, generated Python — and most subcommands reach one. `0` is rejected. |
| `-V`, `--version` | Print the version. |
| `-h`, `--help` | Print help. |

Beyond those, project selection and JSON output are per-command flags; pass them
to the leaf command.

### Environment

| Variable | Effect |
| --- | --- |
| `SFUMATO_BROWSER` | Path to the Chromium-family browser to use. Also honours `PUPPETEER_EXECUTABLE_PATH` and `CHROME_PATH`, in that order, before searching `PATH` and well-known locations. |
| `SFUMATO_PLUGIN_REGISTRY_URL` | Overrides the page-plugin registry URL, which defaults to `https://sfumato.sh/page-plugin-registry.json`. When it cannot be reached, the on-disk cache is used, then the copy compiled into the binary — and a warning says so rather than silently serving older metadata. |
| `SFUMATO_DISABLE_BROWSER_SANDBOX` | Disables the browser sandbox for Mermaid rendering. The sandbox is on by default because `mmdc` loads model-written source. |
| `NO_COLOR`, `TERM` | Suppress ANSI colour in human output and progress events. |

The installer reads `SFUMATO_VERSION`, `SFUMATO_BIN_DIR` and
`SFUMATO_NO_MODIFY_PATH`; those affect installation, not the binary.

## `init`

### `sfumato init user`

```text
sfumato init user [--yes] [--force]
```

| Flag | Effect |
| --- | --- |
| `--yes` | Use setup defaults without interactive Inquire questions. |
| `--force` | Replace an existing global configuration after validation. |

Creates schema-v5 global configuration, initial connector/model defaults, and
installs the bundled `sfumato-default` theme when absent. Without `--yes`, the
command asks for the user name, learning styles, initial connector, profile,
and provider model ID. Existing configuration is protected unless `--force` is
present.

### `sfumato init project`

```text
sfumato init project <NAME> [--path <PATH>] [--no-activate]
```

| Input | Default | Effect |
| --- | --- | --- |
| `<NAME>` | Required | Portable registry name for the project. |
| `--path <PATH>` | `.` | Existing or intended project working root. |
| `--no-activate` | Off | Register without replacing the globally active project. |

Creates `<PATH>/.sfumato/project.toml`, registers the canonical root globally,
and activates it unless disabled. Duplicate names and unsafe/invalid paths are
rejected.

## `config`

Scopes are `user`, `project`, and `effective`. `effective` is read-only and
represents merged global/project values plus resolution. A `--project` value is
meaningful for project/effective access; omission resolves the active project.

### `sfumato config show`

```text
sfumato config show [KEY] [--scope <user|project|effective>] [--project <NAME>]
```

- `KEY` is an optional dotted key such as `defaults.text`, `page.ui`, or
  `generation_tools.image_gen`.
- `--scope` defaults to `effective`.
- Without `KEY`, prints the complete selected scope as redacted TOML.
- Secret values are never returned.

### `sfumato config set`

```text
sfumato config set <KEY> <VALUE> [--scope <user|project>] [--project <NAME>]
```

- `--scope` defaults to `user`.
- `VALUE` is parsed as TOML first; if parsing fails, it is treated as a plain
  string. Quote shell-sensitive arrays, tables, and strings.
- The complete candidate configuration is validated before an atomic write.
- `effective` is accepted by the shared parser but rejected as a write target.
- Raw credentials cannot be set through this command.

Examples:

```bash
sfumato config set marp.pdf true
sfumato config set page.ui shadcn --scope project --project university
sfumato config set page.plugins '["motion", "threejs"]' --scope project --project university
```

### `sfumato config delete`

```text
sfumato config delete <KEY> [--scope <user|project>] [--project <NAME>]
```

Deletes a dotted key only when the resulting complete configuration remains
valid. `--scope` defaults to `user`; `effective` cannot be written.

## `project`

### `sfumato project list`

```text
sfumato project list
```

Prints a table with active status, registry name, and canonical root path.

### `sfumato project show`

```text
sfumato project show [NAME]
```

Prints the portable project configuration as TOML. Without `NAME`, shows the
active project.

### `sfumato project use`

```text
sfumato project use <NAME>
```

Makes a registered project globally active. This affects commands that omit
`--project`; it does not edit the project's portable configuration.

### `sfumato project remove`

```text
sfumato project remove <NAME>
```

Removes only the global registration. The project root, `.sfumato` directory,
source files, and managed resources remain untouched.

## `connector`

Connector names are user-defined registry keys. Presets are `ollama`,
`lmstudio`, `openrouter`, `anthropic`, `codex`, and `elevenlabs`.

### `sfumato connector list`

```text
sfumato connector list
```

Prints configured connector name, kind, and endpoint/process target.

### `sfumato connector show`

```text
sfumato connector show <NAME>
```

Prints one redacted connector configuration as TOML.

### `sfumato connector capabilities`

```text
sfumato connector capabilities <NAME>
```

Prints native features supported by the connector adapter. These are transport
and management features, not model profile capabilities.

### `sfumato connector models`

```text
sfumato connector models <NAME>
```

Queries the connector's native catalog and prints visible model ID, display
name, input/output modalities, context length, and default marker. It does not
register model profiles.

### `sfumato connector status`

```text
sfumato connector status <NAME>
```

Queries native runtime/account information: Ollama runtime data, OpenRouter key
usage/limits, or Codex account and rate-limit fields where available.

### `sfumato connector presets`

```text
sfumato connector presets
```

Lists the presets available to `connector setup`, with the kind, transport, and
authentication each implies.

### `sfumato connector setup`

```text
sfumato connector setup <ollama|lmstudio|openrouter|anthropic|codex|elevenlabs> \
  [--name <NAME>] \
  [--api-key-env <VARIABLE>]
```

| Input | Effect |
| --- | --- |
| Preset | Required adapter preset. |
| `--name` | Registry name; defaults to the preset name. Allows multiple connectors of one kind. |
| `--api-key-env` | Store an `env:VARIABLE` secret reference rather than an OS-keyring reference. Not used by Codex. |

Setup writes connector metadata but never accepts a raw key argument.

### `sfumato connector login`

```text
sfumato connector login <NAME>
```

Prompts invisibly for a credential and saves it in the operating-system
keyring. The connector must use a stored-secret reference. Codex authentication
is externally managed with `codex login`.

### `sfumato connector auth-status`

```text
sfumato connector auth-status <NAME>
```

Checks whether the configured credential reference can be resolved without
revealing its value. Codex reports that authentication is externally managed.

### `sfumato connector logout`

```text
sfumato connector logout <NAME>
```

Deletes the stored credential while preserving connector and model profiles.

## `model`

Capabilities are `text`, `code`, `image`, `video`, `speech`, and `embedding`.
Option keys and types are listed in
[Connectors and models](03-connectors-and-models.md#supported-model-options).

### `sfumato model add`

```text
sfumato model add <NAME> \
  --connector <CONNECTOR> \
  --id <PROVIDER_MODEL_ID> \
  --capability <CAPABILITY>... \
  [--option <KEY=VALUE>]...
```

All three named arguments are required; `--capability` is repeatable and must
appear at least once. The connector must exist, capability/option combinations
must validate, and the profile name must be unique.

### `sfumato model edit`

```text
sfumato model edit <NAME> \
  [--connector <CONNECTOR>] \
  [--id <PROVIDER_MODEL_ID>] \
  [--capability <CAPABILITY>]... \
  [--option <KEY=VALUE>]...
```

Omitted scalar fields are retained. One or more capability flags replace the
entire set. Options update by key. The edit is rejected if a referenced default
or role would become incompatible.

### `sfumato model list`

```text
sfumato model list
```

Prints profile name, connector, provider model ID, and declared capabilities.

### `sfumato model show`

```text
sfumato model show <NAME>
```

Prints the complete profile and typed options as TOML.

### `sfumato model remove`

```text
sfumato model remove <NAME>
```

Removes an unreferenced profile. User defaults, project defaults, reviewer
roles, and other protected references must be reassigned first.

### `sfumato model use`

```text
sfumato model use <SELECTOR> <PROFILE> [--project <NAME>]
```

`SELECTOR` is a capability or `reviewer`. Without `--project`, writes the global
default. With it, writes the portable project default/role. The profile must
exist and support the selected capability; reviewer requires text.

## `theme`

### `sfumato theme create`

```text
sfumato theme create <NAME>
```

Copies the bundled default package to a new globally reusable theme. Names,
manifest, adapters, and paths are validated; duplicates fail.

### `sfumato theme import`

```text
sfumato theme import <DESIGN_MD_PATH> [--name <NAME>]
```

Imports a Google `DESIGN.md` into a new theme package. `--name` overrides the
name derived from the document.

### `sfumato theme export`

```text
sfumato theme export <NAME> [--out <PATH>]
```

Exports a self-contained Google `DESIGN.md`. Output defaults to `DESIGN.md` in
the current directory.

### `sfumato theme list`

```text
sfumato theme list
```

Lists installed theme package names.

### `sfumato theme show`

```text
sfumato theme show <NAME>
```

Prints the theme manifest, semantic tokens, and adapter paths as TOML.

### `sfumato theme use`

```text
sfumato theme use <NAME> [--project <PROJECT>]
```

Assigns the installed theme to the explicit or active project.

### `sfumato theme regenerate`

```text
sfumato theme regenerate [NAME]
```

Re-derives a theme's renderer stylesheets from its manifest. Without `NAME`,
regenerates every installed theme. Use it after editing a theme's tokens by hand,
or after an upgrade that changes how stylesheets are emitted.

## `template`

Template kinds are `slides`, `page`, and `document`.

### `sfumato template create`

```text
sfumato template create <NAME> --kind <slides|page|document> [--from <PATH>]
```

Creates a reusable structural package. Without `--from`, writes a valid
scaffold; with it, copies and validates a source containing exactly one
`<!-- SFUMATO_CONTENT -->` marker.

### `sfumato template list`

```text
sfumato template list [--kind <slides|page|document>]
```

Lists all templates or filters by resource kind.

### `sfumato template show`

```text
sfumato template show <NAME> [--kind <slides|page|document>]
```

Prints metadata and the complete structural source.

## `artifact`

All artifact commands resolve the active project unless `--project` is passed.

### `sfumato artifact add`

```text
sfumato artifact add <PATH> \
  [--name <NAME>] \
  [--description <TEXT>] \
  [--alt-text <TEXT>] \
  [--tag <TAG>]... \
  [--prompt <IMAGE_RECIPE>] \
  [--theme <THEME> | --all-themes] \
  [--project <PROJECT>]
```

Copies a supported image into `.sfumato/assets`, hashes it, and registers its
metadata/theme variant. `--theme` conflicts with `--all-themes`; omission uses
the project theme. The original file is not modified.

### `sfumato artifact edit`

```text
sfumato artifact edit <NAME> \
  [--description <TEXT>] \
  [--alt-text <TEXT>] \
  [--tag <TAG>]... \
  [--prompt <IMAGE_RECIPE> | --clear-prompt] \
  [--from-theme <THEME> --to-theme <THEME>] \
  [--project <PROJECT>]
```

Supplied tags replace all tags. Prompt setting conflicts with prompt clearing.
Theme reassignment flags require each other.

### `sfumato artifact list`

```text
sfumato artifact list [--project <PROJECT>]
```

Lists project artifact names and summaries.

### `sfumato artifact show`

```text
sfumato artifact show <NAME> [--project <PROJECT>]
```

Prints metadata, variants, hashes, and managed paths.

### `sfumato artifact remove`

```text
sfumato artifact remove <NAME> [--project <PROJECT>]
```

Removes the catalog entry and managed copies, never the original import source.

## `prompt`

Prompt IDs are stable identifiers returned by `prompt list`, not filesystem
paths. Project-aware commands use the active project when `--project` is absent.

### `sfumato prompt list`

```text
sfumato prompt list [--project <PROJECT>]
```

Lists every prompt ID and its resolved bundled/user/project origin.

### `sfumato prompt show`

```text
sfumato prompt show <ID> [--project <PROJECT>]
```

Prints ID, origin, SHA-256 content hash, and resolved template source.

### `sfumato prompt customize`

```text
sfumato prompt customize <ID> \
  --scope <user|project> \
  [--project <PROJECT>]
```

Copies the bundled template to the selected override root. It does not open an
editor. Project scope resolves the explicit or active project.

### `sfumato prompt validate`

```text
sfumato prompt validate [--project <PROJECT>]
```

Strictly validates all independently resolved templates and prints ID, origin,
and abbreviated content hash. Invalid overrides stop instead of falling back.

## `plugin`

Plugin IDs currently include exclusive UI libraries `shadcn`, `materialui`;
utilities `motion`, `threejs`, `theatre`, `lottie`; and internal runtimes
`react`, `react-dom`. Runtime dependencies are installed automatically and are
not directly enabled.

### `sfumato plugin list`

```text
sfumato plugin list [--project <PROJECT>]
```

Lists catalog category/version, installed state/version, and selected project
state.

### `sfumato plugin show`

```text
sfumato plugin show <ID>
```

Shows catalog metadata and description. When installed, it also prints API
global, model usage guidance, runtime hash, and license details.

### `sfumato plugin install`

```text
sfumato plugin install <ID> [--version <VERSION>]
```

Explicitly downloads the catalog recipe and dependencies, verifies pinned
hashes/licenses, and installs an offline package. `--version` must match a
version available in the catalog; generation never downloads plugins.

### `sfumato plugin update`

```text
sfumato plugin update <ID>
```

Reinstalls the current catalog-pinned version and required dependencies.

### `sfumato plugin enable`

```text
sfumato plugin enable <ID> [--project <PROJECT>]
```

Enables an installed utility or selects an installed UI library. Selecting a UI
replaces the previous UI because UI libraries are exclusive.

### `sfumato plugin disable`

```text
sfumato plugin disable <ID> [--project <PROJECT>]
```

Removes a utility from project defaults or clears it when it is the selected UI.

## `tool`

Tool values are `image-gen`, `video-gen`, `audio-gen`, and `chart-gen`. These are
model-facing generation tools, not browser plugins.

### `sfumato tool list`

```text
sfumato tool list [--project <PROJECT>]
```

Reports project default state and whether the required image, video, or speech
model can be resolved.

### `sfumato tool enable`

```text
sfumato tool enable <image-gen|video-gen|audio-gen|chart-gen> [--project <PROJECT>]
```

Persists the tool as enabled for the explicit or active project. Generation
still checks resource compatibility and model availability.

### `sfumato tool disable`

```text
sfumato tool disable <image-gen|video-gen|audio-gen|chart-gen> [--project <PROJECT>]
```

Persists the tool as disabled for the explicit or active project.

## `renderer`

Renderer values are `hyperframe` and `manim`. The command group is named
`renderer`, not `render`.

### `sfumato renderer list`

```text
sfumato renderer list
```

Prints pinned package version, installed state, health, and details for both
managed local video renderers.

### `sfumato renderer install`

```text
sfumato renderer install <hyperframe|manim|pagedjs>
```

Performs the explicit network/package installation. Hyperframe uses npm and
also installs pinned GSAP. Manim uses `uv` and a managed Python 3.12 virtual
environment.

### `sfumato renderer remove`

```text
sfumato renderer remove <hyperframe|manim|pagedjs>
```

Deletes that managed renderer installation. It does not delete generated
resources or system Node/Python/FFmpeg installations.

### `sfumato renderer doctor`

```text
sfumato renderer doctor [hyperframe|manim|pagedjs]
```

Checks one renderer or both when omitted. Reports required and optional native
dependencies, executable/runtime presence, and upstream doctor details.

## `video`

Resolves a Hyperframe review that `generate video --visual-review` paused. Review
identifiers are printed by the paused run and are also visible in the TUI.

### `sfumato video preview`

```text
sfumato video preview <REVIEW_ID> [--project <PROJECT>] [--json]
```

Prints the paused review: the contact sheet path, the scene breakdown, and what
the reviewer flagged. Does not resume or discard anything.

### `sfumato video approve`

```text
sfumato video approve <REVIEW_ID> [--project <PROJECT>] [--out <FOLDER>] [--json]
```

| Flag | Effect |
| --- | --- |
| `--out <FOLDER>` | Override the saved destination and publish under `<FOLDER>/_sfumato/videos`. |

Accepts the paused review and renders the film to completion, then publishes it
like any other finished resource.

## `generate`

All generation instructions are required and non-empty. `[INPUTS]...` accepts
zero or more local files/directories and appears before or after flags as
allowed by Clap. Directories are recursively filtered to supported textual
extensions.

Common model values use `CAPABILITY=PROFILE`. Tool values are `image-gen`,
`video-gen`, `audio-gen`, and `chart-gen`; a tool incompatible with the resource
is rejected. `--review-model` is a profile name.

`[INPUTS]...` accepts zero or more local files and directories. **Under a project
grounded in a Vitruvio brain they are refused, not ignored** — the run is rejected
with an explanation, because silently reading from the brain while appearing to
read the paths given would be worse. Remove them, or set
`knowledge.backend = "filesystem"`.

### `sfumato generate slides`

```text
sfumato generate slides [INPUTS]... \
  --instruction <TEXT> \
  [--title <TITLE>] \
  [--template <NAME>] \
  [--out <FOLDER>] \
  [--project <NAME>] \
  [--brain-project <NAME>] \
  [--brain <NAME>] \
  [--theme <NAME>] \
  [--model <CAPABILITY=PROFILE>]... \
  [--review-model <PROFILE>] \
  [--no-review] \
  [--no-pdf] \
  [--allow-code-execution] \
  [--tool <image-gen|video-gen|audio-gen|chart-gen>]... \
  [--disable-tool <image-gen|video-gen|audio-gen|chart-gen>]... \
  [--dry-run] \
  [--json]
```

| Flag | Meaning |
| --- | --- |
| `--title` | Explicit deck title; otherwise the drafter supplies a content title. |
| `--template` | Opt into one installed slide template; none is used by default. |
| `--out` | Publish the rendered PDF under `<FOLDER>/_sfumato/slides`; managed Markdown/PDF remain in the artifact store. |
| `--project`, `--theme` | One-request project/theme selection. |
| `--brain-project`, `--brain` | One-request Vitruvio project/brain selection, overriding `knowledge.project` and `knowledge.brain`. Refused unless the project is already grounded in a brain. Note `--project` is the Sfumato project and `--brain-project` the Vitruvio one. |
| `--model` | Repeatable capability profile override. Slides require text; image may be needed by the tool. |
| `--review-model` | One-request reviewer profile override. |
| `--no-review` | Skip semantic review, layout inspection, and model layout repair. Core structural validation and final rendering still apply. |
| `--tool`, `--disable-tool` | One-request tool override. Slides support `image-gen` and `chart-gen`; incompatible tools are rejected. |
| `--no-pdf` | Turn PDF export off for this run, whatever `marp.pdf` says. Configuration could previously only turn it on. |
| `--allow-code-execution` | Consent for this run to execute the Python `chart-gen` writes. Required by `chart-gen` on any resource, not just Manim video. |
| `--dry-run` | Resolve and render prompt preview without provider/render/artifact calls. |
| `--json` | Print machine-readable `GenerationOutput`. |

The hidden legacy `--pdf` flag remains accepted for compatibility. PDF is now
controlled by effective project configuration and generated by default in the
v5 workflow; new callers should not use the hidden flag.

See [Generate slides](05-generate-slides.md) for the complete pipeline.

### `sfumato generate document` / `doc` / `docs`

```text
sfumato generate document [INPUTS]... \
  --instruction <TEXT> \
  [--title <TITLE>] \
  [--template <NAME>] \
  [--out <FOLDER>] \
  [--page-size <a4|letter>] \
  [--toc | --no-toc] \
  [--cover | --no-cover] \
  [--project <NAME>] \
  [--brain-project <NAME>] \
  [--brain <NAME>] \
  [--theme <NAME>] \
  [--model <CAPABILITY=PROFILE>]... \
  [--review-model <PROFILE>] \
  [--no-review] \
  [--allow-code-execution] \
  [--tool <image-gen|video-gen|audio-gen|chart-gen>]... \
  [--disable-tool <image-gen|video-gen|audio-gen|chart-gen>]... \
  [--dry-run] \
  [--json]
```

`doc` and `docs` are visible aliases for `document`.

| Flag | Meaning |
| --- | --- |
| `--title` | Explicit document title; otherwise generated from content. |
| `--template` | Opt into one document structural template. No template is implicit. |
| `--out` | Publish under `<FOLDER>/_sfumato/documents`. |
| `--page-size` | Sheet to print on. The theme decides when omitted. |
| `--toc` / `--no-toc` | Force the table of contents on or off for this generation. |
| `--cover` / `--no-cover` | Force the cover page on or off for this generation. |
| `--model`, `--review-model` | Capability and reviewer overrides. |
| `--no-review` | Skip semantic review and page-format repair. Structural validation, Mermaid validation, and printing still apply. |
| `--tool`, `--disable-tool` | Override the image-generation tool. |
| `--dry-run`, `--json` | Preflight or machine-readable output. |

Omitting a page-setup flag is not the same as passing its negative form: an
omitted flag defers to the theme, while `--no-toc` overrides a theme that asks
for one.

See [Generate documents](12-generate-documents.md) for the pipeline, the
structure contract, the cover, and format repair.

### `sfumato generate page` / `pages`

```text
sfumato generate page [INPUTS]... \
  --instruction <TEXT> \
  [--title <TITLE>] \
  [--template <NAME>] \
  [--out <FOLDER>] \
  [--project <NAME>] \
  [--brain-project <NAME>] \
  [--brain <NAME>] \
  [--theme <NAME>] \
  [--model <CAPABILITY=PROFILE>]... \
  [--review-model <PROFILE>] \
  [--ui <ID|none>] \
  [--plugin <UTILITY_ID>]... \
  [--disable-plugin <UTILITY_ID>]... \
  [--no-review] \
  [--allow-code-execution] \
  [--tool <image-gen|video-gen|audio-gen|chart-gen>]... \
  [--disable-tool <image-gen|video-gen|audio-gen|chart-gen>]... \
  [--dry-run] \
  [--json]
```

`pages` is a visible alias for `page`.

| Flag | Meaning |
| --- | --- |
| `--title` | Explicit page title; otherwise generated from content. |
| `--template` | Opt into one page structural template. No template is implicit. |
| `--out` | Publish under `<FOLDER>/_sfumato/pages`. |
| `--ui` | Override the project's exclusive installed UI library; `none` disables it for this request. |
| `--plugin` | Enable an installed utility for this request; repeatable. UI/runtime IDs are rejected here. |
| `--disable-plugin` | Remove a project utility for this request; repeatable. |
| `--model`, `--review-model` | Capability and reviewer overrides. |
| `--no-review` | Skip semantic and browser-repair model stages. Static validation and assembly still apply. |
| `--tool`, `--disable-tool` | Override image, video, and audio generation tools. Page video generation is remote-model-backed and limited to one tool call; `audio-gen` gives the drafter a speech tool that returns an audio file and its word timings. |
| `--dry-run`, `--json` | Preflight or machine-readable output. |

The hidden `--shadcn` compatibility flag maps to `--ui shadcn` and is
deprecated. New callers must use `--ui`.

See [Generate pages](06-generate-pages.md) for assembly, CSP, offline runtime,
inspection, and publication details.

### `sfumato generate video`

```text
sfumato generate video [INPUTS]... \
  [--url <URL>]... \
  --instruction <TEXT> \
  --engine <hyperframe|manim|model> \
  --duration <SECONDS> \
  [--workflow <auto|explainer|motion-graphics|product-launch|talking-head|slideshow|general>] \
  [--title <TITLE>] \
  [--out <FOLDER>] \
  [--project <NAME>] \
  [--brain-project <NAME>] \
  [--brain <NAME>] \
  [--theme <NAME>] \
  [--model <CAPABILITY=PROFILE>]... \
  [--review-model <PROFILE>] \
  [--no-review] \
  [--visual-review] \
  [--resolution <VALUE>] \
  [--aspect-ratio <VALUE>] \
  [--fps <INTEGER>] \
  [--quality <draft|standard|high>] \
  [--audio <auto|on|off>] \
  [--voice <VOICE_ID>] \
  [--allow-code-execution] \
  [--tool <image-gen|audio-gen|chart-gen>]... \
  [--disable-tool <image-gen|audio-gen|chart-gen>]... \
  [--dry-run] \
  [--json]
```

| Flag | Meaning |
| --- | --- |
| `--duration` | Required integer seconds, currently `1..=3600`. |
| `--url` | Repeatable. A page to read as grounding material alongside `[INPUTS]`. Hyperframe only. |
| `--workflow` | Routes scene direction; defaults to `auto`, which picks from the instruction. Hyperframe only. |
| `--visual-review` | Pause after the contact sheet and wait for `sfumato video preview` and `sfumato video approve`. Hyperframe only. |
| `--engine` | Required: local Hyperframe, local Manim, or asynchronous remote model. There is no automatic fallback between engines. |
| `--out` | Publish only the MP4 under `<FOLDER>/_sfumato/videos/<slug>/`. |
| `--resolution` | Defaults to `1080p` locally and `720p` remotely. |
| `--aspect-ratio` | Defaults to `16:9`; validated against engine/provider support. |
| `--fps` | Local engines only; defaults to `30`, valid range `1..=120`. |
| `--quality` | Local engines only; defaults to `high`. |
| `--audio` | Both local engines narrate when a `speech` default exists (`auto`); `on` requires one and fails without it; `off` renders silent. Remote defaults to `auto`. |
| `--voice` | Local engines only. Overrides the speech profile's voice for this film. |
| `--allow-code-execution` | Authorizes generated Python for this request — Manim scenes, and the `chart-gen` tool on any resource. Project `security.allow_python` is the persistent alternative. |
| `--model` | Text drafter plus `code` for local authoring or `video` for remote generation. |
| `--review-model`, `--no-review` | Controls semantic plan review. Invalid local source is still eligible for one focused repair so the renderer contract can be satisfied. |
| `--tool`, `--disable-tool` | Video planning supports `image-gen`, `audio-gen`, and `chart-gen`; standalone video never injects `video-gen`. `audio-gen` turns narration on or off for a local film. |
| `--dry-run`, `--json` | Preflight or machine-readable output. |

Engine-incompatible flags fail before paid provider calls or code execution.
See [Generate videos](07-generate-videos.md).

## `edit`

### `sfumato edit slides`

```text
sfumato edit slides <DECK> \
  --instruction <TEXT> \
  [--project <NAME>] \
  [--model text=<PROFILE>]... \
  [--json]
```

Applies a focused RFC 6902 patch to a generated managed Marp deck, validates
and renders it, then commits a child revision. Only text model overrides are
accepted. It always regenerates the PDF. See
[Editing, TUI, and automation](08-editing-tui-and-automation.md#focused-slide-editing).

## Exit And Output Rules

- Success exits zero; rejected input, resolution, provider, tool, renderer,
  artifact, and cancellation failures exit non-zero.
- Human list commands render ANSI-aware tables when appropriate.
- Generation human progress goes to stderr; final paths go to stdout.
- `--json` generation/edit commands print a result object on success or an
  `{ "error": ... }` object on failure while preserving the non-zero status.
- Configuration/management commands do not currently expose `--json`; agents
  should consume their stable TOML/table/text output or use generation JSON.
