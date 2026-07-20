# Getting Started

## Requirements

Sfumato itself requires a Rust toolchain capable of building the workspace.
Individual resource types have additional optional dependencies:

| Feature | Dependency |
| --- | --- |
| Local Ollama models | A running Ollama installation and downloaded model. |
| OpenRouter models | An OpenRouter API key. |
| Codex models | An installed and authenticated `codex` executable. |
| Slide PDF | Marp CLI and a Chromium-compatible browser. |
| Mermaid in slides | Mermaid CLI (`mmdc`). |
| Browser-inspected pages | Chrome, Chromium, or Edge. |
| Hyperframes video | Managed Hyperframes renderer, Node.js, FFmpeg, FFprobe, and Chrome. |
| Manim video | Managed Manim renderer, `uv`, FFmpeg, and FFprobe. |

## Build And Install

Build every crate:

```bash
cargo build --workspace
```

During development, run the binary directly:

```bash
target/debug/sfumato --help
```

Install the current checkout into Cargo's binary directory:

```bash
cargo install --path . --force
```

Alternatively, keep a development symlink pointing to the debug binary. Rebuild
after every code change so the symlink resolves to the new executable:

```bash
ln -sf "$(pwd)/target/debug/sfumato" ~/.local/bin/sfumato
cargo build --workspace
sfumato --help
```

Ensure `~/.cargo/bin` or `~/.local/bin` is present in `PATH` when using those
installation methods.

## Initialize The User

Interactive setup:

```bash
sfumato init user
```

Non-interactive setup with defaults:

```bash
sfumato init user --yes
```

Replace an existing global configuration:

```bash
sfumato init user --yes --force
```

`init user` creates schema-v5 global configuration, installs the bundled
`sfumato-default` theme when absent, and establishes initial connector/model
defaults. Without `--yes`, Inquire prompts collect the user name, learning
styles, connector choice, and initial model information. `--force` is required
before replacing an existing global configuration.

## Register A Project

A project is a named pointer to a working directory such as an Obsidian vault,
course directory, or work repository.

```bash
sfumato init project university --path "/path/to/Obsidian Vault"
```

This command:

1. resolves and validates the project root;
2. creates `<project-root>/.sfumato/project.toml`;
3. registers the name and root in the global project registry;
4. selects `sfumato-default` as the initial theme;
5. activates the project unless `--no-activate` is passed.

Register without changing the active project:

```bash
sfumato init project work --path ~/Documents/work --no-activate
```

Confirm the result:

```bash
sfumato project list
sfumato project show university
sfumato config show --scope effective --project university
```

## Configure A Text Model

### Ollama

```bash
sfumato connector setup ollama
sfumato connector status ollama
sfumato connector models ollama
sfumato model add local-text \
  --connector ollama \
  --id gemma3:latest \
  --capability text \
  --capability code
sfumato model use text local-text --project university
sfumato model use code local-text --project university
```

### OpenRouter

```bash
sfumato connector setup openrouter
sfumato connector login openrouter
sfumato connector auth-status openrouter
sfumato model add cloud-text \
  --connector openrouter \
  --id openai/gpt-4.1-mini \
  --capability text \
  --capability code
sfumato model use text cloud-text --project university
```

### Codex App Server

```bash
codex login
sfumato connector setup codex
sfumato connector models codex
sfumato model add codex \
  --connector codex \
  --id default \
  --capability text \
  --capability code
sfumato model use text codex --project university
sfumato model use code codex --project university
```

Codex authentication remains owned by the Codex application. Do not run
`sfumato connector login codex`.

## First Generation

Resolve everything without calling a provider:

```bash
sfumato generate slides \
  --project university \
  --instruction "Explain Fourier series visually" \
  --dry-run
```

Generate for real:

```bash
sfumato generate slides \
  --project university \
  --instruction "Explain Fourier series visually"
```

Add grounding material by placing files or directories after the subcommand:

```bash
sfumato generate slides ./notes ./course-material \
  --project university \
  --instruction "Create revision slides grounded in these notes"
```

For automation, add `--json` and read the artifact paths from stdout:

```bash
sfumato generate slides ./notes \
  --project university \
  --instruction "Create revision slides" \
  --json
```

## TUI Versus Explicit Commands

Run `sfumato` without arguments to open the Ratatui interface. It offers nested
views for projects, connectors, models, themes, plugins, prompts, and generation.
The generation form changes by resource: slide-only PDF controls do not appear
for pages or videos, page UI/plugin controls do not appear for slides, and video
engine controls change when the selected engine changes.

Use explicit commands when:

- an agent or script invokes Sfumato;
- arguments must be reproducible in shell history;
- JSON output is required;
- stdin/stdout must remain non-interactive;
- the process is running without a terminal.

## Next Steps

- Read [Configuration and projects](02-configuration-and-projects.md) before
  editing TOML or relying on precedence.
- Read [Connectors and models](03-connectors-and-models.md) before adding image
  or video models.
- Read the guide for the intended resource before performing a paid generation.
