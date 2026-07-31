# Generate Pages

## Command

```bash
sfumato generate page [INPUTS]... \
  --instruction <text> \
  [--title <title>] \
  [--template <name>] \
  [--out <folder>] \
  [--project <name>] \
  [--theme <name>] \
  [--model <capability=profile>]... \
  [--review-model <profile>] \
  [--ui <id|none>] \
  [--plugin <utility-id>]... \
  [--disable-plugin <utility-id>]... \
  [--tool image-gen|video-gen|audio-gen]... \
  [--disable-tool image-gen|video-gen|audio-gen]... \
  [--no-review] \
  [--dry-run] \
  [--json]
```

`pages` is a visible alias for `page`.

## Flags

| Flag | Meaning |
| --- | --- |
| `[INPUTS]...` | Optional textual grounding files/directories using the shared source allowlist and budgets. |
| `--instruction` | Required page teaching objective. |
| `--title` | Optional explicit resource title. |
| `--template` | Opt into one installed page structure. Omission means no template. |
| `--out` | Publish the Obsidian-facing page tree. |
| `--project` | Override active project. |
| `--theme` | Override project theme. The theme must have a valid HTML adapter. |
| `--model` | Repeatable model override, commonly `text=`, `image=`, or `video=`. |
| `--review-model` | Override semantic/browser repair profile. |
| `--ui` | Select one installed UI library or `none` to disable the project UI for this request. |
| `--plugin` | Add one installed utility plugin for this request; repeatable and deduplicated. |
| `--disable-plugin` | Remove one project utility for this request. |
| `--tool` | Enable `image-gen`, `video-gen`, or `audio-gen` for this request. |
| `--disable-tool` | Disable one project/default tool for this request. |
| `--no-review` | Skip semantic review and browser-focused model repair. Static validation still applies. |
| `--dry-run` | Resolve/render without provider, browser, or artifact writes. |
| `--json` | Emit structured output only. |

The hidden `--shadcn` compatibility flag maps to `--ui shadcn` and emits a
deprecation warning. New callers must use `--ui`.

## UI, Utilities, And Runtimes

Only one UI library may be selected. `shadcn` and `materialui` cannot coexist.
Command `--ui` overrides project `page.ui`; `--ui none` disables it temporarily.

Utilities such as `motion`, `threejs`, `theatre`, and `lottie` combine with one
another. Runtime dependencies such as React are resolved transitively and are
not user-selectable plugins.

Install before generation:

```bash
sfumato plugin install shadcn
sfumato plugin install motion
sfumato plugin enable shadcn --project university
sfumato plugin enable motion --project university
```

Per-request selection:

```bash
sfumato generate page \
  --instruction "Build an interactive Fourier explorer" \
  --ui shadcn \
  --plugin motion \
  --plugin threejs
```

Installed assets are hash-verified and inlined. The final page does not fetch
scripts, styles, fonts, images, media, or modules from the network.

## Structured Model Contract

The page model does not return an entire HTML document. It returns strict data:

```json
{
  "title": "Fourier Series Explorer",
  "body_html": "<section>...</section>",
  "css": "#sfumato-page { ... }",
  "javascript": "..."
}
```

The body must be a semantic fragment, not `<html>`, `<head>`, `<body>`,
`<style>`, or `<script>`. CSS and JavaScript belong only in their dedicated
fields. Remote URLs, CSS imports, module imports, traversal, inline script tags,
and references to unregistered assets are rejected.

## Pipeline

1. Resolve project, theme, models, template, UI, utilities, tools, and output.
2. Require the theme's HTML shell, CSS, and optional JavaScript adapter.
3. Load `SFUMATO.md`, textual sources, and reusable project artifacts.
4. Render strict page prompts with plugin usage guidance.
5. Draft structured page fragments.
6. Apply one semantic RFC 6902 review unless disabled.
7. Validate HTML with `html5ever`, CSS with `lightningcss`, and JavaScript with `oxc_parser`.
8. Assemble theme shell, selected template, CSS, runtimes, and scripts.
9. Add an offline Content Security Policy.
10. Render math and inspect desktop (`1440x900`) and mobile (`390x844`) viewports.
11. Repair browser issues once with focused RFC 6902 patches and accept only improvement.
12. Commit an immutable revision and optionally publish it.

## Image, Video, And Audio Tools

### `image-gen`

When enabled and backed by an image profile, the drafter can request themed
educational images. Sfumato adds theme tokens and project guidance, stores the
image locally, returns a relative path, validates the reference, and discards
unreferenced images before commit.

### `video-gen`

When enabled and backed by a remote `video` profile, the page drafter may make
at most one video-generation call. Hyperframes and Manim are never used as page
tools. Sfumato augments the prompt with theme/artifact context, waits for the
remote result, stores the MP4, and returns a local path suitable for `<video>`.
Unreferenced generated video is discarded.

### `audio-gen`

When enabled and backed by a `speech` profile, the drafter can call
`sfumato_audio_gen` with a line to speak. Sfumato stores the audio locally and
returns its relative path, its spoken length, and a sidecar JSON file of
word-level timings the page can use to synchronise captions or highlighting.

Example:

```bash
sfumato generate page ./notes \
  --instruction "Explain Fourier synthesis with an animation" \
  --ui shadcn \
  --tool image-gen \
  --tool video-gen \
  --tool audio-gen
```

## Math

TeX delimited by `\(...\)` and `\[...\]` activates the pinned offline MathJax
runtime. Sfumato renders TeX to SVG before browser inspection. Remaining raw TeX
is a generation error rather than a silently broken page.

## Theme Assembly

The assembler replaces exactly one `<!-- SFUMATO_CONTENT -->` marker in the
theme shell or selected reusable template. It:

- inlines theme CSS and optional theme JavaScript;
- maps active theme tokens into page/UI semantic variables;
- loads plugin runtimes before generated JavaScript;
- removes duplicate external theme references;
- permits only registered local image/video assets;
- produces a page that opens directly through `file://` without a web server.

## Browser Inspection

Inspection detects:

- uncaught JavaScript exceptions;
- rejected promises;
- missing images or videos;
- blank primary content;
- unprocessed math;
- horizontal overflow on desktop/mobile.

Runtime errors or missing assets remaining after repair fail the transaction.
Responsive overflow may commit with an explicit warning. If no browser is
available, static validation may still commit the page with a warning.

## Artifacts And Publication

Managed revision:

```text
~/.sfumato/Projects/<project>/resources/pages/<resource-id>/
├── current.json
└── revisions/<revision-id>/
    ├── manifest.json
    ├── index.md
    ├── index.html
    └── assets/
        ├── images/
        └── videos/
```

Published tree:

```text
<out>/_sfumato/pages/<slug>/
├── index.md
├── index.html
└── assets/
```

`index.md` marks the resource as Sfumato-managed and links to the HTML for
Obsidian. Publication atomically replaces the complete tree and cleans stale
legacy output shapes only after success.

## Dry Run And JSON

Dry run reports selected template, UI/utilities, runtimes, models, reusable
artifacts, tools, planned HTML path, and rendered prompt. It makes no provider,
browser, or write calls.

JSON output includes project, title, HTML path, `SFUMATO.md`, models, plugins,
template, project artifacts, runtimes, tools, managed/published files, review
state, and prompt provenance.
