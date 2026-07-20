# Sfumato CLI

Sfumato is a local-first Rust CLI for generating Obsidian-friendly learning
resources from an instruction and optional source files. It currently generates
Marp slide decks, standalone HTML pages, and MP4 videos.

Sfumato supports Ollama, OpenRouter, and the local Codex App Server. Projects
select reusable themes, model defaults, page plugins, and model-backed tools.
Generated resources are stored as immutable revisions under `~/.sfumato` and
may publish processed PDF, HTML, or MP4 files into an Obsidian vault.

## Quick Start

Build the workspace:

```bash
cargo build --workspace
```

Initialize Sfumato and register a project:

```bash
sfumato init user
sfumato init project university --path /path/to/obsidian/vault
```

Configure a connector and model profile:

```bash
sfumato connector setup ollama
sfumato model add local-text \
  --connector ollama \
  --id gemma3:latest \
  --capability text \
  --capability code
sfumato model use text local-text --project university
```

Generate a resource:

```bash
sfumato generate slides \
  --project university \
  --instruction "Explain Fourier series visually"

sfumato generate page \
  --project university \
  --instruction "Build an interactive Fourier series explorer"

sfumato generate video \
  --project university \
  --engine hyperframe \
  --duration 10 \
  --instruction "Animate Fourier synthesis"
```

Run `sfumato` without a subcommand to open the interactive Ratatui workspace.
Explicit subcommands remain the preferred interface for scripts and agents.

## Documentation

The complete operational documentation lives in [docs/guide](docs/guide/README.md):

- [Getting started](docs/guide/01-getting-started.md)
- [Configuration and projects](docs/guide/02-configuration-and-projects.md)
- [Connectors and models](docs/guide/03-connectors-and-models.md)
- [Themes, templates, artifacts, prompts, plugins, tools, and renderers](docs/guide/04-resource-building-blocks.md)
- [Generating slides](docs/guide/05-generate-slides.md)
- [Generating pages](docs/guide/06-generate-pages.md)
- [Generating videos](docs/guide/07-generate-videos.md)
- [Editing, TUI, and automation](docs/guide/08-editing-tui-and-automation.md)
- [Complete command reference](docs/guide/09-command-reference.md)
- [Troubleshooting](docs/guide/10-troubleshooting.md)

Architecture decisions, diagrams, and internal APIs remain under
[docs/architecture](docs/architecture/README.md), [docs/adr](docs/adr), and
[docs/reference](docs/reference).

## Development

The workspace contains a presentation binary and three library crates:

```text
sfumato                 CLI, TUI, formatting, and composition root
sfumato-domain          pure entities and invariants
sfumato-core            workflows, ports, and application facade
sfumato-adapters        filesystem, HTTP, prompts, renderers, and stores
```

Run the standard checks with:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo doc --workspace --no-deps
```
