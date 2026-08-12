# CLI reference

Generated from the clap command declarations by the `renders_the_committed_cli_reference`
test in `cli/tests/unit/cli.rs`. Do not edit by hand: a reference that disagrees with the
parser is worse than none, because it costs a turn to discover which one is lying.

Regenerate with `SFUMATO_WRITE_CLI_REFERENCE=1 cargo test -p sfumato cli_reference`.

Every command also accepts the global `--timeout <SECONDS>`, which abandons the operation
after that many seconds and is unbounded when omitted. `--help` is omitted throughout.

## `sfumato artifact`

Manage reusable project artifacts such as logos and icons


### `sfumato artifact add` `<PATH>` `--name` `--description` `--alt-text` `--tag` `--prompt` `--theme` `--all-themes` `--project`

Copy and register a reusable project artifact


### `sfumato artifact edit` `<NAME>` `--description` `--alt-text` `--tag` `--prompt` `--clear-prompt` `--from-theme` `--to-theme` `--project`

Edit artifact metadata or reassign a themed variant


### `sfumato artifact list` `--project`

List reusable artifacts for a project


### `sfumato artifact remove` `<NAME>` `--project`

Remove a reusable artifact without touching its original source


### `sfumato artifact show` `<NAME>` `--project`

Show one reusable project artifact


## `sfumato config`

Show and edit Sfumato configuration


### `sfumato config delete` `<KEY>` `--scope` `--project`

Delete a config value by dotted key

- `--scope`: user, project, effective

### `sfumato config set` `<KEY>` `<VALUE>` `--scope` `--project`

Set a config value by dotted key

- `--scope`: user, project, effective

### `sfumato config show` `[KEY]` `--scope` `--project`

Show the effective, user, or project config

- `--scope`: user, project, effective

## `sfumato connector`


### `sfumato connector auth-status` `<NAME>`

Check whether a connector credential is available


### `sfumato connector capabilities` `<NAME>`

Show native features exposed by a connector


### `sfumato connector list`


### `sfumato connector login` `<NAME>`

Securely save a connector credential in the operating-system keyring


### `sfumato connector logout` `<NAME>`

Remove a connector credential from secure storage


### `sfumato connector models` `<NAME>`

Discover models available through a connector's native catalog


### `sfumato connector presets`

List the connector presets available to `connector setup`


### `sfumato connector setup` `<PRESET>` `--name` `--api-key-env`

- `PRESET`: ollama, lmstudio, openrouter, anthropic, codex, elevenlabs

### `sfumato connector show` `<NAME>`


### `sfumato connector status` `<NAME>`

Show native account, usage, or local runtime status


## `sfumato edit`

Edit existing generated resources without regenerating them


### `sfumato edit slides` `<MARKDOWN_PATH>` `--instruction` *(required)* `--project` `--model` `--json`


## `sfumato generate`


### `sfumato generate document` `[INPUTS]` `--instruction` *(required)* `--title` `--template` `--out` `--page-size` `--toc` `--no-toc` `--cover` `--no-cover` `--allow-code-execution` `--dry-run` `--project` `--brain-project` `--brain` `--theme` `--model` `--review-model` `--no-review` `--json` `--tool` `--disable-tool`

- `--page-size`: a4, letter
- `--tool`: image-gen, video-gen, audio-gen, chart-gen
- `--disable-tool`: image-gen, video-gen, audio-gen, chart-gen

### `sfumato generate page` `[INPUTS]` `--instruction` *(required)* `--title` `--template` `--out` `--allow-code-execution` `--dry-run` `--project` `--brain-project` `--brain` `--theme` `--model` `--review-model` `--plugin` `--disable-plugin` `--ui` `--shadcn` *(hidden)* `--no-review` `--json` `--tool` `--disable-tool`

- `--tool`: image-gen, video-gen, audio-gen, chart-gen
- `--disable-tool`: image-gen, video-gen, audio-gen, chart-gen

### `sfumato generate slides` `[INPUTS]` `--instruction` *(required)* `--title` `--template` `--out` `--pdf` *(hidden)* `--no-pdf` *(hidden)* `--allow-code-execution` `--dry-run` `--project` `--brain-project` `--brain` `--theme` `--model` `--review-model` `--no-review` `--json` `--tool` `--disable-tool`

- `--tool`: image-gen, video-gen, audio-gen, chart-gen
- `--disable-tool`: image-gen, video-gen, audio-gen, chart-gen

### `sfumato generate video` `[INPUTS]` `--url` `--instruction` *(required)* `--title` `--engine` *(required)* `--workflow` `--duration` *(required)* `--out` `--dry-run` `--project` `--brain-project` `--brain` `--theme` `--model` `--review-model` `--no-review` `--visual-review` `--json` `--resolution` `--aspect-ratio` `--fps` `--quality` `--audio` `--voice` `--allow-code-execution` `--tool` `--disable-tool`

- `--engine`: hyperframe, manim, model
- `--workflow`: auto, explainer, motion-graphics, product-launch, talking-head, slideshow, general
- `--audio`: auto, on, off
- `--tool`: image-gen, video-gen, audio-gen, chart-gen
- `--disable-tool`: image-gen, video-gen, audio-gen, chart-gen

## `sfumato init`


### `sfumato init project` `<NAME>` `--path` `--no-activate`


### `sfumato init user` `--yes` `--force`


## `sfumato model`


### `sfumato model add` `<NAME>` `--connector` *(required)* `--id` *(required)* `--capability` *(required)* `--option`


### `sfumato model edit` `<NAME>` `--connector` `--id` `--capability` `--option`


### `sfumato model list`


### `sfumato model remove` `<NAME>`


### `sfumato model show` `<NAME>`


### `sfumato model use` `<SELECTOR>` `<PROFILE>` `--project`


## `sfumato plugin`

Install and configure offline page plugins


### `sfumato plugin disable` `<ID>` `--project`

Disable a plugin for a project


### `sfumato plugin enable` `<ID>` `--project`

Enable an installed plugin for a project


### `sfumato plugin install` `<ID>` `--version`

Download and install a page plugin


### `sfumato plugin list` `--project`

List all supported page plugins


### `sfumato plugin show` `<ID>`

Show metadata and model guidance for a page plugin


### `sfumato plugin update` `<ID>`

Update an installed page plugin


## `sfumato project`


### `sfumato project list`


### `sfumato project remove` `<NAME>`


### `sfumato project show` `[NAME]`


### `sfumato project use` `<NAME>`


## `sfumato prompt`

Inspect and customize model prompt templates


### `sfumato prompt customize` `<ID>` `--scope` *(required)* `--project`

Copy a bundled prompt into an editable override

- `--scope`: user, project

### `sfumato prompt list` `--project`

List available prompt template IDs


### `sfumato prompt show` `<ID>` `--project`

Show the resolved source for a prompt template


### `sfumato prompt validate` `--project`

Validate all resolved prompt templates


## `sfumato renderer`

Install and diagnose local renderers


### `sfumato renderer doctor` `[RENDERER]`

- `RENDERER`: hyperframe, manim, pagedjs

### `sfumato renderer install` `<RENDERER>`

- `RENDERER`: hyperframe, manim, pagedjs

### `sfumato renderer list`


### `sfumato renderer remove` `<RENDERER>`

- `RENDERER`: hyperframe, manim, pagedjs

## `sfumato template`

Manage reusable structural generation templates


### `sfumato template create` `<NAME>` `--kind` *(required)* `--from`

Create a reusable template package

- `--kind`: slides, page, document

### `sfumato template list` `--kind`

List installed reusable templates

- `--kind`: slides, page, document

### `sfumato template show` `<NAME>` `--kind`

Show a reusable template and its structural source

- `--kind`: slides, page, document

## `sfumato theme`


### `sfumato theme create` `<NAME>`


### `sfumato theme export` `<NAME>` `--out`

Export a theme as a Google DESIGN.md file


### `sfumato theme import` `<PATH>` `--name`

Import a Google DESIGN.md file as a theme


### `sfumato theme list`


### `sfumato theme regenerate` `[NAME]`

Re-derive a theme's renderer stylesheets from its manifest


### `sfumato theme show` `<NAME>`


### `sfumato theme use` `<NAME>` `--project`


## `sfumato tool`

Configure optional model-facing generation tools


### `sfumato tool disable` `<TOOL>` `--project`

- `TOOL`: image-gen, video-gen, audio-gen, chart-gen

### `sfumato tool enable` `<TOOL>` `--project`

- `TOOL`: image-gen, video-gen, audio-gen, chart-gen

### `sfumato tool list` `--project`


## `sfumato video`

Preview or approve a paused Hyperframe video review


### `sfumato video approve` `<REVIEW_ID>` `--project` `--out` `--json`


### `sfumato video preview` `<REVIEW_ID>` `--project` `--json`

