# Sfumato CLI

Sfumato is a Rust CLI for generating Obsidian-friendly study resources from an
instruction and optional source material. It is designed to work directly from
the terminal or as a resource-generation engine orchestrated by tools such as
Claude Code.

The current class diagram is available in
[docs/class-diagram.mmd](docs/class-diagram.mmd).

Run `sfumato` without arguments to open the interactive command launcher. Its
nested menus collect command arguments and execute the same commands exposed by
Clap:

```bash
cargo run
```

## Mental Model

- **User:** one global learning profile.
- **Projects:** many registered study/work contexts, with one active project.
- **Themes:** globally reusable design packages selected by projects.
- **Connectors:** named OpenAI-compatible API connections such as Ollama and
  OpenRouter.
- **Model profiles:** named connector models with capabilities such as `text`,
  `code`, `image`, `video`, `speech`, and `embedding`.
- **Generation requests:** a required instruction plus optional files/folders.

Global preferences, connectors, model profiles, and defaults live in:

```text
~/.config/sfumato/config.toml
```

The project registry and active project live in:

```text
~/.config/sfumato/projects.toml
```

Each project keeps portable settings in:

```text
<project-root>/.sfumato/project.toml
```

Reusable themes live in:

```text
~/.config/sfumato/themes/<theme-name>/
```

## OpenAI-Compatible Connectors

Ollama and OpenRouter use the same OpenAI-compatible connector implementation.
They differ only by configuration:

```bash
cargo run -- connector setup ollama
cargo run -- connector setup openrouter --api-key-env OPENROUTER_API_KEY
cargo run -- connector list
cargo run -- connector show openrouter
```

Ollama defaults to `http://localhost:11434/v1` with the required-but-ignored
`ollama` API key. OpenRouter defaults to `https://openrouter.ai/api/v1`, reads
its bearer token from `OPENROUTER_API_KEY`.

## Models

Model profiles are registered globally and reference a connector. A connector
may provide many models:

```bash
cargo run -- model add local-gemma \
  --connector ollama \
  --id gemma4:e2b-mlx \
  --capability text \
  --capability code \
  --option temperature=0.4 \
  --option max_tokens=4000

cargo run -- model add cloud-gemini \
  --connector openrouter \
  --id google/gemini-2.5-flash \
  --capability text \
  --capability code

cargo run -- model add openrouter-image \
  --connector openrouter \
  --id openai/gpt-image-1 \
  --capability image \
  --option quality=high \
  --option output_format=png

cargo run -- model list
cargo run -- model show local-gemma
cargo run -- model edit local-gemma --id gemma4:e2b-mlx --option temperature=0.2
cargo run -- model remove local-gemma
```

`model edit` changes only supplied fields. Repeated `--capability` values replace
the capability set, while repeated `--option key=value` values merge into the
existing options by key.

Choose user and project defaults by capability:

```bash
cargo run -- model use text local-gemma
cargo run -- model use text cloud-gemini --project university
cargo run -- model use image openrouter-image --project university
```

Model resolution order remains:

1. generation command `--model capability=profile`
2. selected project capability default
3. user capability default

## Setup

Create or reset the global user configuration:

```bash
cargo run -- init user
cargo run -- init user --yes --force
```

Create, register, and activate a project:

```bash
cargo run -- init project university --path /path/to/vault
```

Manage projects:

```bash
cargo run -- project list
cargo run -- project show
cargo run -- project show university
cargo run -- project use university
cargo run -- project remove university
```

Removing a project only removes it from the registry; it does not delete project
files.

## Themes

Every project selects one reusable theme. Create, inspect, and assign themes:

```bash
cargo run -- theme create gruvbox
cargo run -- theme list
cargo run -- theme show gruvbox
cargo run -- theme use gruvbox
cargo run -- theme use gruvbox --project university
```

`sfumato init user` installs the bundled `sfumato-default` theme. A custom theme
contains semantic color/font tokens and renderer adapters:

```text
~/.config/sfumato/themes/gruvbox/
├── theme.toml
├── marp/theme.css
└── html/
    ├── page.html
    ├── style.css
    └── script.js
```

The HTML adapter defines the contract for future HTML resources. Its shell must
contain exactly one `<!-- SFUMATO_CONTENT -->` placeholder.

## Generate Slides

Instructions are required. Sources are optional:

```bash
cargo run -- generate slides --instruction "Explain Fourier series visually"
cargo run -- generate slides --instruction "Create revision slides" ./notes ./course-material
cargo run -- generate slides --instruction "Summarize these notes" --project university ./notes
```

Override a model profile for a capability:

```bash
cargo run -- generate slides \
  --instruction "Explain ownership in Rust" \
  --model text=cloud-text
```

Temporarily override the selected project's theme:

```bash
cargo run -- generate slides \
  --instruction "Explain ownership in Rust" \
  --theme gruvbox
```

Generated decks include a copied theme CSS artifact under
`slides/themes/<theme-name>.css`. Normal generation writes both Marp Markdown
and a PDF. PDF export passes the copied CSS for the configured project theme
directly to Marp:

```text
marp --theme <output>/slides/themes/<theme-name>.css <deck.md> -o <deck.pdf>
```

If Marp CLI is not installed, Sfumato keeps the Markdown and theme CSS artifacts
and prints a clear PDF export warning.

If Marp needs an explicit browser executable, configure it in the global or
project Marp settings:

```toml
[marp]
pdf = false
browser_path = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
```

Sfumato uses `browser_path` first, then tries common local Chromium browser
locations, then lets Marp use its own defaults.

During generation, Sfumato declares read-only filesystem tools to compatible
models. The model can ask Sfumato to list directories or read UTF-8 text files,
but tool execution is restricted to the active project root and any source paths
passed to the command.

When the selected project has an `image` model default, slide generation also
declares `sfumato_image_gen`. The drafter supplies a concrete educational image
prompt, while Sfumato adds the selected theme's semantic colors and fonts before
calling the connector's image endpoint. Generated images are stored under
`slides/images/`, returned to the drafter as relative Markdown paths, and listed
as generation artifacts. The reviewer does not receive this tool.

Slide generation also allows Mermaid diagrams. The text model may return fenced
`mermaid` blocks; before writing the deck, Sfumato renders each diagram to SVG,
stores the `.mmd` source and `.svg` output as local artifacts under
`slides/diagrams/`, and replaces the Mermaid block with a relative Markdown
image reference. PDF export enables Marp local-file access so those generated SVG
artifacts render into the final PDF.

Diagram rendering uses local Mermaid CLI (`mmdc`), so it works offline once
`@mermaid-js/mermaid-cli` is installed:

```bash
npm install -g @mermaid-js/mermaid-cli
```

Mermaid SVGs are rendered with the selected Sfumato project theme. Sfumato maps
theme color and font tokens into Mermaid's `base` theme variables before calling
`mmdc`, so diagrams visually match the Marp deck.

Normal generation prints live progress to stderr while the model is working,
including model request rounds, tool calls, compact tool results, and tool
errors. JSON output mode keeps this progress stream disabled so stdout remains
machine-readable.

Tool exploration is bounded. Sfumato defaults to 8 filesystem tool rounds, then
asks the model to stop calling tools and return the final deck. You can tune this
per model profile:

```bash
cargo run -- model edit grok-latest --option max_tool_rounds=12
```

Preview the prompt without calling a connector:

```bash
cargo run -- generate slides --instruction "Explain Fourier series" --dry-run
```

Dry runs print the tools that would be injected into the model request before
the prompt preview:

```text
Injected tools:
- sfumato_list_directory: List files and directories inside the allowed Sfumato project/source roots.
- sfumato_read_file: Read a UTF-8 text file inside the allowed Sfumato project/source roots.
- sfumato_image_gen: Generate a themed educational image and save it beside the slide deck.
```

Return machine-readable output for agent callers:

```bash
cargo run -- generate slides --instruction "Explain Fourier series" --json
```

Successful JSON includes the selected project, selected model profiles, and
declared tool summaries, and artifact paths:

```json
{
  "project": "university",
  "models": {
    "text": "local-text"
  },
  "tools": [
    {
      "name": "sfumato_list_directory",
      "description": "List files and directories inside the allowed Sfumato project/source roots."
    },
    {
      "name": "sfumato_read_file",
      "description": "Read a UTF-8 text file inside the allowed Sfumato project/source roots."
    }
  ],
  "artifacts": [
    "/path/to/vault/Resources/Sfumato/slides/explain-fourier-series.md"
  ]
}
```

Errors are also emitted as JSON when `--json` is set.

## Configuration

Show merged effective configuration:

```bash
cargo run -- config show
cargo run -- config show model_defaults.text
```

Edit user configuration:

```bash
cargo run -- config set defaults.text cloud-text
cargo run -- config set models.cloud-text.model openai/gpt-4o-mini
```

Edit the active or named project:

```bash
cargo run -- config set output_dir "Resources/Sfumato" --scope project
cargo run -- config set model_defaults.text cloud-text --scope project --project university
cargo run -- config delete model_defaults.text --scope project --project university
```

The effective scope is merged and read-only. Resolution order is:

1. command `--model capability=profile` override
2. selected project model default
3. user model default

Theme resolution order is:

1. command `--theme` override
2. selected project's `theme`

Unknown themes and themes without a valid Marp adapter fail clearly rather than
silently falling back.

Sfumato automatically migrates the previous project-theme fields into the
project-owned theme schema. Before replacing a migrated TOML file, it writes a
`.bak` copy beside it.

## Development

The repository is a Cargo workspace. The root `sfumato` package owns terminal
presentation and command routing; `crates/sfumato-core` owns configuration,
repositories, application services, providers, rendering, and generation.

```bash
cargo fmt
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace
```
