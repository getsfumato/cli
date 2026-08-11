# Sfumato

A local-first Rust CLI that turns one instruction plus your own material into a
finished learning resource. It currently generates **Marp slide decks**,
**paginated documents** (PDF via Paged.js), **standalone interactive HTML pages**,
and **MP4 films**.

It works from *your* material, so you get your own notes explained rather than a
model's summary of the topic. Every run is kept as an immutable revision under
`~/.sfumato`, and the processed artifact can be published back beside the notes it
came from — commonly an Obsidian vault.

## Install

```bash
curl -fsSL https://sfumato.sh/install.sh | sh
```

macOS and Linux, Apple Silicon and x86_64. The installer downloads a prebuilt
binary, verifies its checksum, and installs to `~/.local/bin`. Overrides:
`SFUMATO_VERSION`, `SFUMATO_BIN_DIR`, `SFUMATO_NO_MODIFY_PATH`.

From source instead — needs Rust 1.91 or newer:

```bash
cargo install sfumato --locked
```

### Prerequisites

The binary is self-contained; rendering shells out. Nothing is needed until you
generate the thing that needs it, and `sfumato renderer doctor` reports what is
missing.

| Tool | Needed for |
| --- | --- |
| Chrome, Chromium or Edge | slide layout inspection, page inspection, document measurement |
| `marp` | slide decks |
| `mmdc` (mermaid-cli) | Mermaid diagrams |
| `pagedjs-cli` | documents (`sfumato renderer install pagedjs`) |
| `node` and `npm` | Hyperframe videos, managed renderers |
| `ffmpeg`, `ffprobe` | video assembly |
| `uv` | Manim videos, the `chart-gen` tool |

A browser found anywhere on `PATH` is used automatically; otherwise set
`marp.browser_path` in configuration or `SFUMATO_BROWSER` in the environment.

## Quick start

```bash
sfumato init user
sfumato connector setup ollama
sfumato model add local-text \
  --connector ollama --id gemma3:latest \
  --capability text --capability code
sfumato model use text local-text
sfumato init project university --path /path/to/obsidian/vault
```

Then generate, from the files you hand it:

```bash
sfumato generate slides    --instruction "Explain Fourier series visually" notes/fourier.md
sfumato generate document  --instruction "A study guide on Fourier series" notes/
sfumato generate page      --instruction "An interactive Fourier explorer" notes/fourier.md
sfumato generate video     --engine hyperframe --duration 30 \
                           --instruction "Animate Fourier synthesis" notes/fourier.md
```

`--dry-run` plans without spending a token. `--json` makes every command
machine-readable for agents. Run `sfumato` with no subcommand for the interactive
workspace; explicit subcommands stay the better interface for scripts.

## Models

Connectors are configured per preset and models are selected per capability
(`text`, `code`, `image`, `video`, `speech`, `embedding`), so a run can draft with
a local model and illustrate with a remote one.

`ollama` · `lmstudio` · `openrouter` · `anthropic` · `codex` (local Codex app
server) · `elevenlabs`

## Grounding: files or a brain

By default the model browses the sources you pass, through read-only tools scoped
to the project.

Alternatively, ground a project in a **[Vitruvio](https://github.com/getsfumato/vitruvio)
brain** and the model interrogates typed, content-addressed evidence with
provenance instead of reading a directory:

```toml
# .sfumato/project.toml
[knowledge]
backend = "vitruvio"
brain = "my-brain"
```

Vitruvio is a separate project (`pip install vitruvio`); Sfumato talks to its CLI,
so there is no service to run. It is entirely optional and off by default. Under a
brain, source paths are refused rather than ignored — see
[configuration](docs/guide/02-configuration-and-projects.md#choosing-a-knowledge-source).

## Documentation

The complete operational documentation lives in [docs/guide](docs/guide/README.md):

- [Getting started](docs/guide/01-getting-started.md)
- [Configuration and projects](docs/guide/02-configuration-and-projects.md)
- [Connectors and models](docs/guide/03-connectors-and-models.md)
- [Themes, templates, artifacts, prompts, plugins, tools, and renderers](docs/guide/04-resource-building-blocks.md)
- [Generating slides](docs/guide/05-generate-slides.md)
- [Generating pages](docs/guide/06-generate-pages.md)
- [Generating videos](docs/guide/07-generate-videos.md)
- [Generating documents](docs/guide/12-generate-documents.md)
- [Editing, TUI, and automation](docs/guide/08-editing-tui-and-automation.md)
- [Complete command reference](docs/guide/09-command-reference.md)
- [Troubleshooting](docs/guide/10-troubleshooting.md)
- [Hyperframe troubleshooting](docs/guide/11-hyperframe-troubleshooting.md)

Architecture decisions, diagrams, and internal APIs are under
[docs/architecture](docs/architecture/README.md), [docs/adr](docs/adr), and
[docs/reference](docs/reference). Releasing is documented in
[RELEASING.md](RELEASING.md).

## Crates

The workspace is four crates, all published. The three libraries carry no
presentation concerns, so another front end — an API, a service — can reuse the
workflows without this repository:

```toml
sfumato-core = "0.3"
```

| Crate | Contents |
| --- | --- |
| [`sfumato`](https://crates.io/crates/sfumato) | CLI, TUI, formatting, and the composition root |
| [`sfumato-adapters`](https://crates.io/crates/sfumato-adapters) | providers, renderers, stores, prompts, secrets |
| [`sfumato-core`](https://crates.io/crates/sfumato-core) | workflows, ports, and the application facade |
| [`sfumato-domain`](https://crates.io/crates/sfumato-domain) | pure entities and invariants |

Dependencies point inward only — `sfumato` → `adapters` → `core` → `domain` — and
`cli/tests/architecture.rs` enforces it rather than leaving it to discipline.

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
cargo doc --workspace --no-deps --locked
```

The default suite is deterministic, offline and credential-free. Tests that
execute real renderers are opt-in behind the `real-renderers` feature and must
stay that way; CI has a guard that refuses any invocation enabling them. See
[the testing contract](docs/reference/testing.md).

## Licence

MIT. See [LICENSE](LICENSE).
