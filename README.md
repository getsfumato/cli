# Sfumato CLI

Sfumato is a Rust CLI for generating Obsidian-friendly study resources from an
instruction and optional source material. It is designed to work directly from
the terminal or as a resource-generation engine orchestrated by tools such as
Claude Code.

The current class diagram is available in
[docs/class-diagram.mmd](docs/class-diagram.mmd).

Run `sfumato` without arguments to open the Ratatui workspace:

```bash
cargo run
```

The workspace provides nested setup, project, model, connector, theme, and
configuration views with in-place forms and actions. Slide generation uses an
in-terminal request form and a live pipeline view with stage progress, readable
model/tool activity, warnings, artifact paths, and previews for generated
images when the terminal supports them. Screen transitions use TachyonFX; the
header and scrollable activity surfaces use `tui-widgets`.

Explicit Clap commands remain available for scripts, agents, shell history, and
machine-readable `--json` output. They do not enter the alternate-screen TUI:

```bash
sfumato project list
sfumato generate slides --instruction "Explain Fourier series visually" --json
```

## Editing Generated Slides

Use `edit slides` to update the content of an existing generated deck without
asking the model to regenerate its Marp document, title, theme, or slide order:

```bash
sfumato edit slides \
  ~/.sfumato/Projects/university/resources/slides/fourier-series/revisions/<revision-id>/deck.md \
  --instruction "Clarify the geometric explanation on slide four"
```

The active project's text model is used by default. Override it with the same
named-profile syntax used by generation:

```bash
sfumato edit slides ./deck.md \
  --instruction "Correct the transform convention" \
  --project university \
  --model text=cloud-draft \
  --json
```

Sfumato parses the existing deck into its structured slide document, sends the
instruction and slide snapshot to the model, and accepts only revision-guarded
RFC 6902 replacements of individual slide Markdown. Frontmatter, the title
slide, deck title, IDs, and order cannot be changed by this workflow. After
validating the patch, Sfumato checks layout, renders any new Mermaid diagrams,
updates the original `.md`, and replaces the PDF beside it. Markdown and PDF
replacement happens only after Marp successfully renders the edited candidate,
so a rendering failure preserves the original pair.

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

Projects may also define model guidance in the registered project root:

```text
<project-root>/SFUMATO.md
```

Slide generation loads this optional UTF-8 file before collecting sources and
injects it into the drafter, title recovery, semantic reviewer, focused layout
repairer, and image-generation prompts. Sfumato reads only the file at the exact
project root, limits it to 64 KiB, and keeps it separate from source material.
Dry-run and JSON output report the loaded instruction path.

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

The HTML adapter provides the shell, CSS, and optional JavaScript used by standalone pages. Its shell must
contain exactly one `<!-- SFUMATO_CONTENT -->` placeholder.

## Generate Pages

Generate a themed standalone learning page from an instruction and optional
text sources:

```bash
sfumato generate page \
  --instruction "Explain Fourier series interactively"

sfumato generate page ./notes \
  --instruction "Build a revision explorer" \
  --plugin threejs \
  --plugin motion \
  --theme gruvbox \
  --out "/path/to/published/pages"
```

Discover the bundled offline plugin catalog with `sfumato plugin list` and
`sfumato plugin show <id>`. The initial exact packages are Three.js `0.184.0`,
Motion `12.42.2`, Theatre.js Core `0.7.2`, and lottie-web `5.13.0`. Repeated
`--plugin <id>` flags are deduplicated and resolved deterministically. Their
hash-verified runtimes are inlined into the final document under
`window.SfumatoPlugins`, so generation never runs npm or fetches a CDN.

When page content contains TeX delimited by `\(...\)` or `\[...\]`, Sfumato
automatically embeds the pinned MathJax `3.2.2` TeX-to-SVG runtime. Math is
rendered offline before browser layout measurements, and unprocessed TeX is a
generation error rather than a silently broken artifact.

The model returns structured `title`, `body_html`, `css`, and `javascript`
fragments. Sfumato validates them with HTML, CSS, and JavaScript parsers,
assembles the selected theme's HTML adapter, applies an offline CSP, and checks
the result in a local Chromium browser at desktop and mobile sizes. Semantic
and browser fixes are revision-guarded RFC 6902 patches rather than full-page
regeneration.

Managed page revisions live at:

```text
~/.sfumato/Projects/<project>/resources/pages/<resource-id>/
├── current.json
└── revisions/<revision-id>/
    ├── manifest.json
    ├── index.md
    ├── index.html
    └── assets/
        └── images/
```

`--out <folder>` always publishes one predictable Obsidian-facing tree:

```text
<folder>/_sfumato/pages/<slug>/
├── index.md
├── index.html
└── assets/images/
```

The generated `index.md` identifies the resource as Sfumato-managed and links
to the interactive HTML. Publication atomically replaces the complete page
directory and removes stale legacy `<slug>.html` or `<slug>/` outputs only after
the new tree succeeds. Use `--dry-run`, `--json`, `--no-review`,
`--review-model`, `--model text=<profile>`, and the normal project/theme options
the same way as slide generation.

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

Generated decks are immutable, versioned resources:

```text
~/.sfumato/Projects/<project>/resources/slides/<resource-id>/
├── current.json
└── revisions/<revision-id>/
    ├── manifest.json
    ├── deck.md
    ├── deck.pdf
    ├── images/
    ├── diagrams/
    └── themes/
```

Every generation writes into a private staging transaction and publishes the
revision only after its manifest and declared files validate. Failed or
cancelled operations remove staging. The registered project path remains a
source root apart from its portable `.sfumato/project.toml`, `SFUMATO.md`, and
optional prompt overrides.

PDF export passes the copied CSS for the configured project theme directly to
Marp:

```text
marp --theme <revision>/themes/<theme-name>.css <revision>/deck.md -o <revision>/deck.pdf>
```

If Marp CLI is not installed, Sfumato keeps the Markdown and theme CSS artifacts
and prints a clear PDF export warning.

Use `--out` to publish the processed PDF and a small Obsidian index while
retaining the complete generation workspace centrally:

```bash
cargo run -- generate slides \
  --instruction "Explain Fourier series visually" \
  --out "/path/to/Obsidian/Published Slides"
```

The published resource uses the same visible namespace as generated pages:

```text
<out>/_sfumato/slides/<slug>/
├── index.md
└── <slug>.pdf
```

`index.md` links to and embeds the PDF in Obsidian. Regeneration atomically
replaces this published folder; immutable Markdown, diagrams, images, themes,
and revision history remain under Sfumato's central artifact workspace. After
the new folder commits successfully, Sfumato removes the legacy loose
`<out>/<slug>.pdf` publication.

A persistent project publication destination can be configured with
`publish_dir`. Relative values are resolved from the registered project root.

If Marp needs an explicit browser executable, configure it in the global or
project Marp settings:

```toml
[marp]
pdf = false
browser_path = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
```

Sfumato uses `browser_path` first, then tries common local Chromium browser
locations, then lets Marp use its own defaults.

Text connectors must finish with a complete response. If an OpenAI-compatible
connector reports `finish_reason: length` or `max_tokens`, or rejects a request
because its context window is full, Sfumato rejects the partial response and
starts one stage-specific compact recovery request:

- Draft recovery distributes a bounded evidence digest across the supplied
  files, trims project guidance, disables additional tools, and asks for a
  complete deck of at most 14 slides.
- Semantic-review recovery sends a bounded deck snapshot and source digest,
  disables tools, and requests at most three high-impact RFC 6902 changes.
  Slides with truncated Markdown are explicitly protected from replacement.
- Layout recovery sends only the overflowing slide, measured issue, and trimmed
  project guidance, requesting a direct replacement under 1,200 tokens.

The terminal reports the original and compacted prompt sizes when recovery
starts. JSON output exposes the result as `review.context_compaction`. If the
single compact retry also fails, Sfumato preserves the normalized draft or last
valid reviewed deck and reports the remaining issue; truncated Markdown is
never published as a valid artifact.

During generation, Sfumato declares read-only filesystem tools to compatible
models. The model can ask Sfumato to list directories or read UTF-8 text files,
but tool execution is restricted to the active project root and any source paths
passed to the command.

When the selected project has an `image` model default, slide generation also
declares `sfumato_image_gen`. The drafter supplies a concrete educational image
prompt, while Sfumato adds the selected theme's semantic colors and fonts before
calling the connector's image endpoint. Generated images are stored under the
revision's `images/` directory, returned to the drafter as relative Markdown
paths, and listed as generation artifacts. Sfumato adds a `height:420px` Marp constraint when the
drafter does not provide one, preserving the original full-resolution image. If
the composed slide still overflows, the local layout check sends only that slide
to the focused reviewer, which may reduce the image height or split the content;
the repair is accepted only when a second browser measurement improves it. The
reviewer does not receive the image-generation tool.

Slide generation also allows Mermaid diagrams. The text model may return fenced
`mermaid` blocks; before writing the deck, Sfumato renders each diagram to SVG,
stores content-addressed `.mmd` sources and `.svg` outputs under the revision's
`diagrams/` directory, and replaces the Mermaid block with a relative Markdown
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

Successful JSON distinguishes internal workspace artifacts from published
processed artifacts:

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
    "/Users/alex/.sfumato/Projects/university/resources/slides/fourier-series/revisions/rev-123/deck.md",
    "/Users/alex/.sfumato/Projects/university/resources/slides/fourier-series/revisions/rev-123/deck.pdf",
    "/Users/alex/.sfumato/Projects/university/resources/slides/fourier-series/revisions/rev-123/manifest.json"
  ],
  "published_artifacts": [
    "/path/to/vault/Published Slides/explain-fourier-series.pdf"
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
cargo run -- config set publish_dir "Published Slides" --scope project
cargo run -- config delete publish_dir --scope project
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

Configuration schema v4 is a deliberate reset. Legacy and future schema
versions fail read-only with an actionable error; run `sfumato init user
--force` and recreate project registrations instead of relying on an implicit
migration. Writes validate the complete document and replace it atomically.
Credentials are stored only as indirect references such as
`env:OPENROUTER_API_KEY`.

## Prompts

Model-facing language lives in MiniJinja Markdown templates rather than Rust
source. Resolve, inspect, validate, or customize templates with:

```bash
sfumato prompt list
sfumato prompt show slides.draft.user
sfumato prompt customize slides.review.user --scope user
sfumato prompt customize slides.layout-repair.user --scope project
sfumato prompt validate
```

Each template resolves independently from project override, user override, then
the bundled package. Existing invalid overrides stop before model invocation.
Generated manifests record every prompt ID, origin, schema version, and SHA-256
source hash used by the workflow.

## Development

The repository is a Cargo workspace. The root `sfumato` package owns terminal
presentation and composition, `sfumato-domain` owns pure invariants,
`sfumato-core` owns workflows and ports, and `sfumato-adapters` owns MiniJinja,
HTTP, and transactional filesystem implementations.

```bash
cargo fmt
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace
cargo doc --workspace --no-deps
```
