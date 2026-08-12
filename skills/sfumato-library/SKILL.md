---
name: sfumato-library
description: The reusable pieces a sfumato generation draws on — themes, structural templates, project artifacts, prompt overrides, page plugins and model-facing tools. Use when changing how a resource looks, adding a logo or figure, customizing prompt wording, installing a page library, or enabling image, video, audio or chart generation.
allowed-tools: Bash(sfumato:*), Read, Glob, Grep
---

# The library a generation draws on

Six independently managed concepts. They are routinely confused, so start here:

| Concept | Scope | Decides |
|---|---|---|
| **Theme** | global package, one selected per project | How everything looks. Colours, fonts, one renderer adapter per resource kind. |
| **Template** | global package, opt-in per run | Structure a resource is poured into. Never implicit. |
| **Artifact** | per project | A reusable visual — a logo, a figure — the model may place. |
| **Prompt** | bundled, overridable per user or project | The words sent to the model. |
| **Plugin** | global install, enabled per project | A JavaScript library a generated **page** may use. |
| **Tool** | enabled per project | A function the **model** may call during drafting. |

A theme is not a template. A page plugin is not a tool. A tool is not a renderer.

## Themes

Exactly one per project, and there is no silent fallback: a missing theme or a
missing adapter for the resource being generated is an error.

```text
<platform-config>/sfumato/themes/<name>/
├── theme.toml          # semantic colours, fonts, relative adapter paths
├── marp/theme.css      # must declare Marpit theme metadata
└── html/               # page.html with exactly one <!-- SFUMATO_CONTENT -->
```

```console
sfumato theme create gruvbox
sfumato theme use gruvbox --project university
sfumato theme export gruvbox --out ./DESIGN.md
sfumato theme import ./ferrari-DESIGN.md --name ferrari
sfumato theme regenerate gruvbox
```

`import` consumes a Google DESIGN.md, validates the normative colour and
typography tokens, keeps the source guidance, and builds the adapters. `export`
writes one back. Together they are how a theme moves between a design tool and
Sfumato without hand-transcribing tokens.

`regenerate` re-derives renderer stylesheets from the manifest — reach for it
after hand-editing a `theme.toml`, or after an upgrade that changed how a
stylesheet is derived. Omitting the name does every installed theme.

A document theme declares its page setup, and every part is optional:

```toml
[adapters.document]
css = "document/print.css"
page_size = "a4"
table_of_contents = true
cover = true
cover_image = "document/mark.svg"
```

A theme installed before documents existed still validates and falls back to a
bundled print stylesheet derived from its own tokens.

## Structural templates

Optional, and applied **only** when a generation passes `--template <name>`. The
structural source contains exactly one `<!-- SFUMATO_CONTENT -->`; the drafter
returns content for the marker and Sfumato does the merge before validation.

```console
sfumato template create lecture --kind slides
sfumato template create lecture --kind slides --from ./existing.md
sfumato template list --kind document
sfumato template show lecture
```

Kinds are `slides`, `page`, and `document`. `show` does not need `--kind` — the
package declares its own, and requiring it would mean you could not inspect a
package to find out what it is.

## Project artifacts

A reusable logical visual with semantic metadata and one or more theme variants,
under `<project-root>/.sfumato/assets/`. PNG, JPEG, WebP, GIF, and passive SVG —
SVG validation rejects scripts and unsafe external behaviour.

```console
sfumato artifact add ./figures/spectrum.png \
  --name square-wave-spectrum \
  --description "Odd-harmonic square-wave spectrum" \
  --alt-text "Amplitude bars at odd harmonic frequencies" \
  --tag fourier --tag spectrum \
  --prompt "Recreate the labeled spectrum with the same data" \
  --theme gruvbox --project university
```

Variant selection prefers the exact active theme, then the wildcard `*`.
`--prompt` is the recipe an image model uses to reconstruct a missing variant, so
an artifact can follow a project that changes theme. `--theme` and `--all-themes`
conflict.

Only artifacts the final reviewed resource actually references are copied into
its revision.

```console
sfumato artifact edit spectrum --from-theme gruvbox --to-theme nord
sfumato artifact list --project university
sfumato artifact remove spectrum --project university
```

`--tag` on `edit` **replaces** every tag rather than appending. `remove` deletes
the catalog entry and the managed copies; the original file you added from is
never touched.

## Prompt overrides

All model-facing language is MiniJinja Markdown. Rust owns typed policy,
validators, retry counts, and output contracts; the templates own the prose.

Each prompt ID resolves independently: project override → user override →
bundled. An invalid override **stops the operation before the provider is
called** — Sfumato never falls back past one, because silently using different
words than the ones you edited is worse than failing.

```console
sfumato prompt list --project university          # the authoritative ID list
sfumato prompt show slides.draft.user --project university
sfumato prompt customize slides.draft.user --scope project --project university
# edit the created Markdown
sfumato prompt validate --project university
```

`customize` copies and opens no editor. `validate` strictly renders every
resolved template against fixture values and reports origin and hash. Run it
after every edit.

Every manifest records each prompt ID, its origin, version, and SHA-256 hash, so
a revision can always answer which words produced it.

## Page plugins

Browser libraries installed from pinned public-CDN recipes, hash- and
licence-verified, stored offline. **Generation never downloads anything** — only
`plugin install` does.

| Category | Rule | IDs |
|---|---|---|
| UI | Zero or one active. Enabling another replaces it. | `shadcn`, `materialui` |
| Utility | Combine freely. | `motion`, `threejs`, `theatre`, `lottie` |
| Runtime | Transitive only; not directly selectable. | `react`, `react-dom` |

```console
sfumato plugin install shadcn
sfumato plugin enable shadcn --project university
sfumato plugin list --project university
```

Per-run: `--ui <id|none>`, `--plugin <utility>`, `--disable-plugin <utility>`.

## Generation tools

Provider-backed functions offered to the drafter. Not page JavaScript.

```console
sfumato tool list --project university
sfumato tool enable chart-gen --project university
```

| Resource | `image-gen` | `video-gen` | `audio-gen` | `chart-gen` |
|---|---|---|---|---|
| Slides | yes | no | no | yes |
| Documents | yes | no | no | yes |
| Pages | yes | yes, remote model, at most once per page | yes | yes |
| Videos | yes, during planning | no | not a tool — enabling it makes the film narrated | yes, during planning |

`tool list` reports both whether the project enables a tool and whether a model
profile actually backs it. Both are required; a tool with no profile is simply
not offered to the model.

### `chart-gen` is the one to prefer for anything quantitative

It has no provider and costs no remote call. The drafter writes matplotlib
statements and Sfumato runs them locally, which means a chart's numbers are
**computed rather than imagined**. Sfumato owns the imports, the headless
backend, the theme styling, and the save, so a chart matches the resource it sits
in without being told the palette.

Because it executes generated Python it needs the tool enabled **and**
`security.allow_python`. Missing either leaves it off the model's list entirely
rather than failing mid-draft:

```console
sfumato tool enable chart-gen --project university
sfumato config set security.allow_python true --scope project --project university
```

Packages beyond matplotlib, numpy, and sympy go in `security.python_packages`.

### Bounds worth knowing

The filesystem read tools are internal, read-only, mandatory, and scoped to the
project and supplied source roots — they never appear in tool config. Listings
are sorted and capped at 200 entries with a `truncated` marker; a `read_file`
call is capped at 128 KiB and **rejects** an oversized file rather than silently
abbreviating it. Generated images are capped at 64 MiB and page videos at 512
MiB. Generated filenames are content-addressed, and unreferenced generated media
is removed before the final commit.
