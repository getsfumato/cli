# Sfumato CLI

Sfumato is a Rust CLI for generating Obsidian-friendly study resources from an
instruction and optional source material. It is designed to work directly from
the terminal or as a resource-generation engine orchestrated by tools such as
Claude Code.

## Mental Model

- **User:** one global learning profile.
- **Projects:** many registered study/work contexts, with one active project.
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

The old v0.1 single-project/single-inference config format is intentionally not
supported. Run `sfumato init user --force` and register projects again.

## Development

```bash
cargo fmt
cargo test
```
