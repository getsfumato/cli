# Sfumato CLI

Sfumato is a Rust CLI for generating Obsidian-friendly study resources with local
or cloud models. The first resource type is a Marp slide deck generated from
files and folders.

## Current MVP

- `sfumato init user` opens a small setup experience, asks a few questions, and
  writes your user preferences.
- `sfumato init project` writes a project preference template.
- `sfumato generate slides <inputs...>` reads supported files, asks a model for a
  Marp deck, and writes Markdown into the configured vault output folder.
- Providers are OpenAI-like chat completion adapters for Ollama and OpenRouter.
- PDF export is optional and uses the external `marp` CLI when available.

Supported input extensions:

```text
.md .txt .rs .py .js .ts .html .css .json .toml .yaml .yml
```

## Examples

```bash
cargo run -- init user
cargo run -- init project
cargo run -- generate slides ./notes --provider ollama --model llama3.2 --title "Intro"
OPENROUTER_API_KEY=... cargo run -- generate slides ./notes --provider openrouter --model openai/gpt-4o-mini
```

Show the merged effective config:

```bash
cargo run -- config show
```

Show, set, or delete a specific user/project value:

```bash
cargo run -- config show user.theme
cargo run -- config set user.theme gruvbox
cargo run -- config set inference.temperature 0.3
cargo run -- config delete user.name
cargo run -- config set project.output_dir "Resources/Sfumato" --scope project
```

Config keys use dotted paths. Values are parsed as TOML when possible, so
`true`, `0.3`, `4000`, and `["visual", "practice"]` become typed values;
otherwise they are saved as strings.

The init flow uses:

- `inquire` for interactive questions.
- `indicatif` for write progress feedback.

Use the starter defaults without the interactive questions:

```bash
cargo run -- init user --yes
```

Overwrite an existing user config:

```bash
cargo run -- init user --force
```

Preview the prompt without calling a model:

```bash
cargo run -- generate slides ./notes --dry-run
```

Export a PDF when Marp CLI is installed:

```bash
cargo run -- generate slides ./notes --pdf
```

If Marp is missing, Sfumato keeps the Markdown deck and reports that PDF export
was skipped.

## Configuration

Config is layered in this order:

1. User config: `~/.config/sfumato/config.toml`
2. Project config: `.sfumato/project.toml`
3. CLI flags for inference-time overrides

Project config example:

```toml
[project]
name = "university"
vault_root = "."
output_dir = "Resources/Sfumato"
```

User config example:

```toml
[user]
name = "Alex"
learning_style = ["visual", "step-by-step"]
theme = "sfumato-default"

[inference]
provider = "ollama"
model = "llama3.2"
temperature = 0.4
max_tokens = 4000

[providers.ollama]
base_url = "http://localhost:11434/v1"
api_key = "ollama"

[providers.openrouter]
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"

[marp]
theme = "default"
pdf = false
```

## Development

```bash
cargo fmt
cargo test
```
