# Generate Documents

A document is prose that flows across numbered pages: a title, hierarchical
sections, an optional cover and table of contents. It is not a deck. Use it for
study notes, handouts, reference sheets, and anything meant to be read or printed
rather than projected.

## Command

```bash
sfumato generate document [INPUTS]... \
  --instruction <text> \
  [--title <title>] \
  [--template <name>] \
  [--out <folder>] \
  [--page-size a4|letter] \
  [--toc | --no-toc] \
  [--cover | --no-cover] \
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

`doc` and `docs` are aliases for `document`.

## Pipeline

1. Resolve the project, theme, models, tools, page setup, and paths.
2. Load textual sources, `SFUMATO.md`, and reusable project artifacts.
3. Draft Markdown prose, retrying once with compacted context on a model limit.
4. Parse the draft into a validated sectioned document; repair the structure once
   if it is invalid.
5. Render and validate every Mermaid diagram; repair one invalid diagram.
6. Apply semantic RFC 6902 review unless `--no-review` is set.
7. Assemble printable HTML, paginate it, and measure page-format defects.
8. Repair defects one section at a time, keeping only repairs that measurably
   improve the document.
9. Print the PDF, commit one immutable revision, and publish when requested.

`--no-review` disables both semantic review and page-format repair. Structural
validation, Mermaid validation, and PDF printing remain mandatory.

## Rendering

Markdown becomes HTML, the theme's print stylesheet is applied, and the Paged.js
CLI paginates and prints it.

Pagination is what CSS Paged Media describes and no browser implements directly:
sheet size, margins, running headers, page numbers, and a table of contents whose
page numbers resolve to where each section actually landed.

Rendering goes through `pagedjs-cli` rather than a browser Sfumato drives itself,
for one reason: pagination and printing have to happen in the same browser
session. Every piece of page furniture — the page number, the running header, a
contents entry's page reference — is resolved from the paginator's own counters.
A browser driven directly prints when it considers the page ready, without knowing
the paginator is still working, which produces a different page count on every
run and, in the worst case, a truncated document. The CLI owns that ordering, the
same way the Marp CLI owns it for decks.

Install the pinned version Sfumato manages:

```bash
sfumato renderer install pagedjs
sfumato renderer doctor pagedjs
```

A global `npm install -g pagedjs-cli` also works. The renderer prefers the managed
copy and falls back to whatever the shell exposes, because pagination output
depends on the paginator's version and a pinned copy keeps one machine's documents
comparable with another's.

The renderer reuses the Chrome that Sfumato already requires rather than a second
browser, passes `--blockRemote` so a document cannot reach the network at render
time, and generates a PDF outline from the document's `h2`–`h4` headings.

## Structure Contract

The drafter returns Markdown that satisfies these rules; a draft that does not is
repaired once and then rejected:

- exactly one `# H1`, as the first line of content, which becomes the title;
- every other heading is `##` or deeper, starting at `##`;
- heading levels never skip a rank, so `##` may be followed by `###` but never
  directly by `####`;
- an optional leading frontmatter block declaring only `subtitle`;
- a short orienting paragraph between the title and the first section.

The heading hierarchy is validated as a whole, because an outline that skips a
rank renders a table of contents that skips a rank.

## Page Setup

The theme sets the defaults and a flag overrides them for one generation.

| Setting | Theme key | Flag | Default |
| --- | --- | --- | --- |
| Sheet | `page_size` | `--page-size a4\|letter` | `a4` |
| Contents | `table_of_contents` | `--toc` / `--no-toc` | on |
| Cover | `cover` | `--cover` / `--no-cover` | on |

A theme declares them under `[adapters.document]`:

```toml
[adapters.document]
css = "document/print.css"
page_size = "a4"
table_of_contents = true
cover = true
cover_image = "document/mark.svg"   # optional
```

Every part is optional, and so is the whole block: a theme installed before
documents existed keeps validating, and falls back to a bundled print stylesheet
derived from that theme's own colour and font tokens.

## Cover Page

Sfumato composes the cover, never the model. It carries the title, the subtitle
when the document declares one, the project name, and a date; the theme decides
how it looks, and may add a cover image.

The date comes from the revision's own timestamp rather than from the clock, so
the same revision always reproduces the same PDF.

The cover is its own unnumbered page and the body numbering starts after it. The
prompts explicitly forbid the model from writing a title page, a contents list, or
page references, because Sfumato generates all three from the document's
structure and a hand-written copy would duplicate them.

## Format Repair

A slide overflows because it is a fixed box. Prose reflows, so this stage looks
for the defects that survive pagination instead:

| Defect | Meaning |
| --- | --- |
| `overflows_text_column` | Content is wider than the text column, typically a wide table or an unbroken string. |
| `taller_than_page` | A single block cannot fit one page no matter where it breaks. |
| `orphaned_heading` | A heading sits at the foot of a page with its body on the next. |
| `nearly_empty_page` | An unsplittable block pushed itself to a fresh page and left the previous one almost blank. |

Defects are measured on the paginated markup the renderer emits, so they describe
the pages the PDF actually contains.

Each repair rewrites one section and the document is re-measured. A repair is
kept only when the defect count and total severity both improve, so a rewrite that
trades one defect for another is discarded rather than applied. A section that
cannot be improved is abandoned, and any defect that survives is reported in the
warnings and in `--json` output.

## Math, Diagrams, And Images

Math uses dollar delimiters, `$inline$` and `$$display$$`. LaTeX-style `\(...\)`
is rejected on purpose: CommonMark reads `\(` as an escaped parenthesis and would
consume the delimiter before any renderer sees the formula. Math is parsed, then
handed to a vendored MathJax that is embedded only when the document contains
math.

The printable HTML embeds no paginator of its own: the renderer's CLI injects one,
and a second copy would paginate the document twice.

Mermaid fences become themed SVGs through the same renderer slides use. A
document constrains a diagram by the width of its text column, where a slide
constrains it by the height of its fixed box.

`image-gen` works exactly as it does for slides. Only referenced assets survive
into the committed revision, and markup that references an asset which was never
generated fails at assembly rather than printing a broken box.

## Artifacts

Managed revision:

```text
~/.sfumato/Projects/<project>/resources/documents/<resource-id>/
├── current.json
└── revisions/<revision-id>/
    ├── manifest.json
    ├── <slug>.md
    ├── <slug>.html
    ├── <slug>.pdf
    ├── diagrams/
    └── images/
```

Publication:

```text
<out>/_sfumato/documents/<slug>/<slug>.pdf
```

The Markdown source, the printable HTML, and the revision history stay managed and
are not copied to the publication directory.

## Dry Run And JSON

A dry run resolves the project, theme, page setup, models, tools, template, and
artifacts, and prints the rendered drafting prompt. It calls no model, browser, or
artifact store.

`--json` reports the project, models by role, tools, page setup, embedded
runtimes, every committed file, the published PDF, review and repair statuses,
remaining format defects, prompt provenance, and warnings.
