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
`slides/themes/<theme-name>.css`. PDF export passes this CSS directly to Marp.

Preview the prompt without calling a connector:

```bash
cargo run -- generate slides --instruction "Explain Fourier series" --dry-run
```

Return machine-readable output for agent callers:

```bash
cargo run -- generate slides --instruction "Explain Fourier series" --json
```

Successful JSON includes the selected project, selected model profiles, and
artifact paths. Errors are also emitted as JSON when `--json` is set.

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
