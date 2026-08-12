# The artifact store

Where a generated resource actually lives, why there are two copies of it, and
which one to read.

## Managed revisions are authoritative

Every successful generation commits one **immutable revision**:

```text
~/.sfumato/Projects/<project>/resources/<kind>/<resource-id>/
├── current.json
└── revisions/<revision-id>/
    ├── manifest.json
    └── <the resource and everything it references>
```

`<kind>` is `slides`, `documents`, `pages`, or `videos`. Nothing overwrites a
previous revision: an edit or a regeneration writes a new one and moves the
pointer in `current.json`. A failed or cancelled transaction is staged and then
removed, so it never becomes current.

`manifest.json` records the project, the revision's lineage, every model profile
used and for what role, every prompt ID with its origin, version, and SHA-256
hash, the tools that were offered, and every committed file. It is the answer to
"which model and which prompt produced this", months later.

Per resource:

```text
slides/…/       deck.md, deck.pdf, images/, diagrams/, themes/
documents/…/    <slug>.md, <slug>.html, <slug>.pdf, diagrams/, images/
pages/…/        index.md, index.html, assets/images/, assets/videos/
videos/…/       <slug>.mp4, plan.json, storyboard.md, source/, assets/
                narration.json when the film speaks
```

A video's `source/` holds what the model actually authored — `meta.json` and
`index.html` for Hyperframes, `scene.py` for Manim. That is what a focused
repair rewrites, and what you read to understand why a film looks the way it
does.

## Publication is a copy for humans

Requested with `--out <folder>` or a project `publish_dir`:

```text
<folder>/_sfumato/slides/<slug>/     index.md + <slug>.pdf
<folder>/_sfumato/documents/<slug>/  <slug>.pdf
<folder>/_sfumato/pages/<slug>/      index.md + index.html + assets/
<folder>/_sfumato/videos/<slug>/     <slug>.mp4
```

`index.md` marks the resource as Sfumato-managed and links to the rendered file
so Obsidian shows it inline. Source Markdown, page HTML source, video plans and
storyboards, and all revision history stay managed and are **not** published.

Publication replaces the tree atomically and cleans stale output only after
success. It happens after the managed commit, which is the ordering that matters
when something fails: a publication error means the revision exists and is
valid.

## Reading paths, not building them

Take them from the `--json` result:

| Field | What it holds |
|---|---|
| `artifacts` | Every committed file in the managed revision. |
| `published_artifacts` | Every file written to the publication directory. |
| `markdown_path`, `pdf_path` | Slides and `edit slides`. |
| `html_path` | Pages. |
| `video_path` | Videos. |

Constructing a revision path by hand breaks the first time a run produces a new
revision — which is every run, including a successful edit. This is the single
most common way an automation goes stale.

## Editing does not mutate

`sfumato edit slides` takes a deck path inside the managed store, applies a
constrained JSON Patch, and commits a **new** revision linked to its parent. The
revision you pointed at is untouched. Read `markdown_path` and `pdf_path` out of
the result — the edited deck is somewhere new.

The deck must be inside the selected project's managed artifact root. An
arbitrary Markdown file in the vault is rejected on purpose: the editor relies on
the deck being one Sfumato generated, with a known title slide and a theme that
still exists.
