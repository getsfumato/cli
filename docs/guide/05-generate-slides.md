# Generate Slides

## Command

```bash
sfumato generate slides [INPUTS]... \
  --instruction <text> \
  [--title <title>] \
  [--template <name>] \
  [--out <folder>] \
  [--project <name>] \
  [--theme <name>] \
  [--model <capability=profile>]... \
  [--review-model <profile>] \
  [--no-review] \
  [--tool image-gen] \
  [--disable-tool image-gen] \
  [--dry-run] \
  [--json]
```

`--instruction` is required. Inputs are optional local files or directories.

## Source Inputs

Directories are traversed recursively. Supported extensions are:

```text
.md .txt .rs .py .js .ts .html .css .json .toml .yaml .yml
```

Preflight limits are deterministic:

- maximum 256 supported files;
- maximum 1 MiB read from each file;
- maximum 16 MiB aggregate preflight budget;
- duplicate canonical paths are read once;
- files are sorted by canonical path;
- unsupported extensions are skipped;
- invalid paths fail explicitly;
- content is reduced to the longest valid UTF-8 prefix after the per-file byte
  cap, so an invalid/truncated suffix is not injected into a prompt.

The prompt does not carry this material. It carries an **index** of it — a
directory tree naming every supplied file with its size — and the model reads
what it needs through the read-only filesystem tools, scoped to the project and
the explicitly supplied source roots.

That is a deliberate inversion of the obvious design. Inlining every source
spends the context window before the model has read a word, forces each file to
be truncated to fit, and — because the agent loop resends the conversation on
every tool round — pays for the whole dump again on each round. None of that
cost buys relevance: most files supplied to a request have nothing to do with
it. Pointing at the corpus and letting the model open the handful it needs makes
the cost track the request instead of the vault.

The one exception is the compacted retry, which runs after a context limit was
already hit and drops the tools; it inlines bounded excerpts, because it has no
way to go and read anything.

## Flags

| Flag | Meaning |
| --- | --- |
| `[INPUTS]...` | Optional grounding files/directories. |
| `--instruction` | Required teaching/transformation request. |
| `--title` | Explicit artifact title. Without it, the drafter produces a proper title independent of the instruction wording. |
| `--template` | Opt into one installed slide template. No template is used when omitted. |
| `--out` | Publish processed PDF and Obsidian index; managed source remains central. |
| `--project` | Override the active project. |
| `--theme` | Override the project's theme for this request. |
| `--model` | Repeatable capability-to-profile override, commonly `text=...` and `image=...`. |
| `--review-model` | Override the reviewer profile. |
| `--no-review` | Skip semantic review, layout inspection, and reviewer layout repair. |
| `--tool image-gen` | Enable image generation for this request. |
| `--disable-tool image-gen` | Disable project/default image generation. |
| `--dry-run` | Resolve and render prompt/tool plans without provider or artifact writes. |
| `--json` | Emit machine-readable output on stdout and suppress live human progress. |

The CLI accepts `video-gen` as a shared tool enum, but slide generation does not
expose a video tool. Use `image-gen` only for slides.

## Examples

Instruction only:

```bash
sfumato generate slides \
  --instruction "Explain Fourier series visually"
```

Grounded sources and explicit project:

```bash
sfumato generate slides ./notes ./course-material \
  --project university \
  --instruction "Create exam revision slides grounded in these files"
```

Explicit drafter/reviewer/theme:

```bash
sfumato generate slides ./notes \
  --instruction "Teach numerical root finding" \
  --theme gruvbox \
  --model text=codex \
  --review-model grok-latest
```

Opt into structure and image generation:

```bash
sfumato generate slides \
  --instruction "Explain the Laplace transform" \
  --template lecture \
  --tool image-gen
```

## Pipeline

1. Resolve project, theme, model profiles, reviewer, template, tools, and paths.
2. Load `<project-root>/SFUMATO.md` when present.
3. Discover and budget textual sources.
4. Resolve reusable project artifacts for the selected theme.
5. Render strict external MiniJinja prompts.
6. Draft a complete Marp deck using the text profile.
7. Normalize frontmatter, fences, title slide, images, math, CSS, and separators.
8. Apply semantic review as revision-guarded RFC 6902 deck patches.
9. Render Mermaid fences to themed local SVG files.
10. Render temporary Marp HTML and measure every 1280x720 slide.
11. Repair overflowing slides independently; accept only measurable improvement.
12. Render PDF through Marp when available/configured.
13. Validate and commit one immutable artifact revision.
14. Publish the processed PDF/index tree when a publication root exists.

## Drafter And Reviewer

The drafter uses the resolved `text` profile. The reviewer uses
`--review-model`, project reviewer, global reviewer, or text drafter fallback.

Semantic review does not regenerate the deck. The reviewer receives a structured
deck snapshot and returns RFC 6902 operations constrained by
`ReviewableDocument`. Frontmatter, stable identity, and prohibited structural
fields remain protected.

Layout repair receives only the overflowing slide and measured issue. It may
split or simplify content and adjust generated image dimensions. Sfumato keeps
the repair only when a second browser measurement improves the layout score.

## Context Compaction

When a provider reports output truncation or context exhaustion, Sfumato allows
one stage-specific compact retry:

- draft retry uses bounded evidence and caps deck size;
- semantic review retry requests only a few high-impact patches;
- layout retry receives only one slide and its metrics.

Partial/truncated Markdown is never accepted as a valid deck. JSON output reports
compaction status under `review.context_compaction`.

## Filesystem And Image Tools

Every slide drafter receives:

- `sfumato_list_directory`;
- `sfumato_read_file`.

They are read-only and path-restricted. The default model tool budget is eight
rounds and can be changed through profile `max_tool_rounds`.

When `image-gen` is enabled and an image profile resolves, the drafter also
receives `sfumato_image_gen`. Sfumato augments its prompt with active theme
tokens and project guidance, stores the image in the staging revision, and
returns a relative Markdown path. Unreferenced generated images are discarded.
Unsized generated images receive a bounded Marp height before layout review.

## Mermaid And Math

The drafter may emit fenced Mermaid blocks. Sfumato:

1. validates and normalizes the fence;
2. writes content-addressed `.mmd` source;
3. maps theme tokens to Mermaid variables;
4. invokes local `mmdc`;
5. stores the SVG under `diagrams/`;
6. replaces the fence with a relative image reference.

Mermaid requires local Mermaid CLI. Rendering failures receive one focused
model repair with the actual parser error.

Math uses Marp's MathJax mode. Slide prompts require Marp's dollar delimiters:
`$...$` for inline expressions and `$$...$$` for display expressions. The
normalizer enforces `math: mathjax` and defensively converts paired `\(...\)`
and `\[...\]` delimiters outside code and Mermaid fences before review or
rendering, preserving the TeX expression itself.

## Artifact Layout

Managed immutable revisions:

```text
~/.sfumato/Projects/<project>/resources/slides/<resource-id>/
├── current.json
└── revisions/<revision-id>/
    ├── manifest.json
    ├── deck.md
    ├── deck.pdf              # when rendered
    ├── images/
    ├── diagrams/
    └── themes/
```

The manifest records project, revision lineage, models, prompts, hashes, tools,
and all committed files. Failed staging transactions are removed.

Publication with `--out <folder>` or project `publish_dir`:

```text
<folder>/_sfumato/slides/<slug>/
├── index.md
└── <slug>.pdf
```

The index links to and embeds the PDF for Obsidian. Source Markdown and revision
history are not published.

## Dry Run And JSON

Dry run prints selected project guidance, template, reusable artifacts, tools,
draft/reviewer profiles, planned stages, and the rendered prompt. It performs no
provider, browser, renderer, or artifact-store work.

JSON success includes:

- selected project and `SFUMATO.md` path;
- model profiles by role/capability;
- injected tools and selected template;
- project artifact references;
- managed and published paths;
- review/compaction/layout state;
- prompt provenance.

With `--json`, errors are emitted as structured JSON and the process remains
non-zero.

## Dependency Failure Behavior

- Missing Marp may commit Markdown and theme CSS with a warning, while ensuring
  no stale PDF is presented as current.
- Missing Mermaid CLI fails when the candidate actually contains Mermaid.
- Missing browser skips layout inspection with a warning when the workflow can
  still preserve a valid deck.
- Invalid model output, unknown theme/template, unsafe paths, and invalid source
  material fail before commit.
