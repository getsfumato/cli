---
name: sfumato-generate
description: Generate a deck, document, page or video with sfumato, and read the result honestly. Use when asked to make slides, a handout, an interactive page or a short video from notes, when choosing between the four resources, when a generation produced something wrong, or when a run failed at draft, review, layout, repair or render.
allowed-tools: Bash(sfumato:*), Read, Glob, Grep
---

# Generating a resource

Four resources, one pipeline. What differs is the renderer and therefore the
failure modes; what is shared is worth learning once.

```bash
sfumato generate slides   ./notes --instruction "..." --project university
sfumato generate document ./notes --instruction "..." --project university
sfumato generate page     ./notes --instruction "..." --project university
sfumato generate video    ./notes --instruction "..." --engine hyperframe --duration 45
```

## Choosing between them

Ask what the reader does with it, not what the material is:

- **Presented to a room, one idea at a time** → `slides`. Fixed 1280×720 boxes,
  so overflow is the characteristic defect.
- **Read or printed, prose across numbered pages** → `document`. Reflows, so
  orphaned headings and unbreakable blocks are the defects instead.
- **Interacted with** → `page`. A control, a simulation, something that responds.
  One HTML file plus its local assets, opening over `file://`.
- **Watched** → `video`. Only when the thing genuinely changes over time.

A deck of static diagrams is not a video, and a video someone has to pause to
read is a deck.

## The shared pipeline

1. **Resolve** project, theme, models, reviewer, template, tools, paths.
2. **Load** `<project-root>/SFUMATO.md` when present — freeform project guidance
   that reaches every prompt.
3. **Read sources** — or query the brain. See
   [`sfumato-knowledge`](../sfumato-knowledge/SKILL.md).
4. **Resolve project artifacts** for the active theme.
5. **Render prompts** — strict MiniJinja, no silent fallback past an invalid
   override.
6. **Draft** with the `text` profile.
7. **Validate** structure deterministically.
8. **Review** semantically as an RFC 6902 patch — never a regeneration.
9. **Measure and repair** in a real renderer, keeping a repair only when the
   measurement improves.
10. **Render**, **commit** one immutable revision, **publish** when asked.

Steps 8 and 9 are what `--no-review` removes. Steps 7 and 10 are mandatory, which
is why `--no-review` can produce something unpolished but not something broken.

## Sources

Positional inputs are files or directories, traversed recursively. Allowed
extensions:

```text
.md .txt .rs .py .js .ts .html .css .json .toml .yaml .yml
```

Deterministic preflight limits: at most 256 files, 1 MiB read per file, 16 MiB
aggregate; duplicates read once; sorted by canonical path; unsupported
extensions skipped; invalid paths fail loudly.

**The prompt does not carry the material.** It carries an index — the tree, each
file with its size — and the model opens what it needs with
`sfumato_list_directory` and `sfumato_read_file`, scoped to the project and the
supplied roots. Inlining everything would spend the window before the model read
a word and re-send it on every tool round; pointing at the corpus makes cost
track the request rather than the vault. The one exception is the compacted
retry after a context limit, which has no tools and inlines bounded excerpts.

## The flags that matter

| Flag | Why you would reach for it |
|---|---|
| `--instruction` | Required. The teaching or transformation request. |
| `--title` | Only when you want an exact title. Without it the drafter writes one that is not just the instruction reworded. |
| `--template <name>` | Opt into an installed structure. Never applied implicitly. |
| `--theme <name>` | Try a look without changing the project. |
| `--model text=<profile>` | A stronger drafter for one hard run. Left side is a capability, right side a **profile name**, never a provider model ID. |
| `--review-model <profile>` | A different reviewer. An `image`-capable one unlocks automated frame review on video. |
| `--no-review` | Iterating on wording. Skips semantic review and focused repair. |
| `--tool` / `--disable-tool` | Per-run tool changes without touching project config. |
| `--out <folder>` | Publish beside the notes. Managed revision is committed either way. |
| `--dry-run` | Always, before a first run. |
| `--json` | Always, from a script. |

## Slides

Marp Markdown plus a PDF. Math uses Marp's dollar delimiters — `$inline$` and
`$$display$$`; the normalizer converts stray `\(...\)` and `\[...\]` outside code
and Mermaid fences. Mermaid fences become themed SVGs through local `mmdc`.

The characteristic gate is **layout**: every slide is rendered at 1280×720 in a
real browser and measured. An overflowing slide is repaired on its own, and the
repair is kept only when a second measurement improves the score.

Dependency behaviour worth knowing before you debug the wrong thing:

- Missing Marp — commits Markdown and CSS with a warning, and makes sure no stale
  PDF is presented as current.
- Missing Mermaid CLI — fails only if the deck actually contains Mermaid.
- Missing browser — skips layout inspection with a warning.

## Documents

Markdown, then Paged.js pagination, then a PDF. Pagination goes through
`pagedjs-cli` rather than a browser Sfumato drives, because page numbers, running
headers, and contents entries all resolve from the paginator's counters — driving
a browser directly prints before it has finished and yields a different page
count every run.

```bash
sfumato renderer install pagedjs
```

The drafter's structure contract: exactly one `# H1` first, everything else `##`
or deeper starting at `##`, no skipped heading ranks, optional frontmatter
declaring only `subtitle`, and a short orienting paragraph before the first
section. A draft that violates it is repaired once, then rejected.

Sfumato composes the cover and the contents, never the model — the prompts forbid
writing a title page, a contents list, or page references, because all three are
generated from the structure. The cover date comes from the revision timestamp,
not the clock, so a revision reproduces the same PDF.

Format defects measured on the paginated markup: `overflows_text_column`,
`taller_than_page`, `orphaned_heading`, `nearly_empty_page`. A repair rewrites one
section and is kept only when defect count *and* total severity both improve.

Math uses `$…$` and `$$…$$`. `\(...\)` is rejected deliberately — CommonMark reads
`\(` as an escaped parenthesis and eats the delimiter before any renderer sees it.

## Pages

One HTML file beside an `assets/` directory. Scripts, styles, and fonts are
inlined and nothing is fetched from the network, so it opens over `file://`
without a server.

The model does not return a document; it returns strict data:

```json
{ "title": "...", "body_html": "<section>...</section>", "css": "...", "javascript": "..." }
```

The body is a semantic fragment — no `<html>`, `<head>`, `<body>`, `<style>`, or
`<script>`. Remote URLs, CSS imports, module imports, traversal, and references to
unregistered assets are all rejected. Validation is real parsing: `html5ever`,
`lightningcss`, `oxc_parser`.

Exactly one UI library may be active (`--ui shadcn`, `--ui none` to disable for a
run); utilities (`motion`, `threejs`, `theatre`, `lottie`) combine freely. All
assets are hash-verified and inlined — the finished page fetches nothing.

Browser inspection at 1440×900 and 390×844 detects uncaught exceptions, rejected
promises, missing media, blank content, unprocessed math, and horizontal
overflow. Runtime errors or missing assets that survive repair **fail**;
responsive overflow may commit with a warning.

Math uses `\(...\)` and `\[...\]` here — the opposite of documents, because a page
is HTML rather than CommonMark. Raw TeX left in the output is an error.

## Videos

`--engine` and `--duration` are required and there is no fallback between
engines.

| Engine | Use it for | Needs |
|---|---|---|
| `hyperframe` | Motion graphics, product and explainer films, anything typographic. | `renderer install hyperframe`, Node, FFmpeg, FFprobe, Chrome. |
| `manim` | Explanations that must be *correct* — an integral, a region of convergence. It computes what it draws. | `renderer install manim`, `uv`, FFmpeg; plus `--allow-code-execution` or `security.allow_python`. |
| `model` | A short remote clip from a video model. | A `video` profile. One request, one clip, no stitching. |

For local engines `--model video=...` is invalid; for `model`, `--model code=...`,
`--fps`, and `--quality` are invalid.

Both local engines author **one scene per model call** against a timeline Sfumato
generates itself, and each request is told how the previous beat leaves the frame
— which is what lets a cut read as one continuous move. It is also why a plan with
many short beats is the expensive shape: cost scales with scene count, not
duration.

Repair is focused: a renderer failure naming a scene re-authors that scene, and
the author is shown the renderer's own measurements — the exact pixel overflow,
the missed contrast ratio — beside its previous attempt. Rounds scale between
three and eight with how many faults were reported, and two consecutive rounds
that clear nothing stop the loop.

**The empty-frame gate** measures ink coverage and colour count at every scene
start and midpoint. A scene opening on an empty frame is a defect, because the
cut lands on nothing. Survivors are reported in `review.frame_defects`.

**Narration**, when a `speech` profile exists and `audio-gen` is enabled: Sfumato
speaks each planned line, and then **retimes the film around the voice** — a beat
lasts at least as long as the words over it, so windows stretch and the total
length can grow. `--duration` is a floor, and any change is reported as a warning.
Captions are an overlay in the lower eighth, never a reserved band. `narration.json`
ships in the revision with every word timing.

`--visual-review` pauses before the encode and reports a `review_id`:

```bash
sfumato generate video --engine hyperframe --duration 12 --visual-review \
  --instruction "Present the dashboard in three beats"
sfumato video preview <review-id>
sfumato video approve <review-id>
```

The session remembers the `--out` from generation; pass `--out` to `approve` only
to override it deliberately.

## Reading the result honestly

- **`warnings` carries the things that silently changed the output**: a missing
  PDF, a stretched duration, a surviving frame defect, uncorrected responsive
  overflow. A run with warnings exits zero and looks identical to a clean one.
- **`--no-review` narrows what was checked.** Say so when reporting.
- **A dry run proves resolution, not success.**
- **Take paths from the result**, never by constructing a revision directory.

## When a generation disappoints

| Symptom | Look at |
|---|---|
| Content is thin or generic | The instruction, then whether sources were actually reachable. `--dry-run` prints the source index. |
| Wrong material used | `--project`, and the grounding — a brain-backed project ignores no paths, it refuses them. |
| It fails at `draft` with `context_limit` | Fewer sources, narrower instruction, or a bigger `max_tokens` on the profile. |
| It fails at `render` with `unavailable` | `sfumato renderer doctor`. |
| Slides overflow anyway | Repair ran and could not improve it. Fewer points per slide, or `--template`. |
| A video beat looks empty | `review.frame_defects`, then the scene in `source/`. |
| A page is blank | Browser inspection found no primary content; check the JavaScript in the revision's `index.html`. |
