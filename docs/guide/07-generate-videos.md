# Generate Videos

## Command

```bash
sfumato generate video [INPUTS]... \
  [--url <URL>]... \
  --instruction <text> \
  --engine hyperframe|manim|model \
  [--workflow auto|explainer|motion-graphics|product-launch|talking-head|slideshow|general] \
  --duration <seconds> \
  [--title <title>] \
  [--out <folder>] \
  [--project <name>] \
  [--theme <name>] \
  [--model <capability=profile>]... \
  [--review-model <profile>] \
  [--resolution <480p|720p|1080p>] \
  [--aspect-ratio <W:H>] \
  [--fps <integer>] \
  [--quality draft|standard|high] \
  [--audio auto|on|off] \
  [--allow-code-execution] \
  [--tool image-gen] \
  [--disable-tool image-gen] \
  [--no-review] \
  [--visual-review] \
  [--dry-run] \
  [--json]
```

Engine and duration are required. Sfumato never falls back to another engine.

## Common Pipeline

1. Resolve project, active theme, models, reviewer, image tool, and paths.
2. Load textual sources, `SFUMATO.md`, and reusable project artifacts.
3. Draft a structured `VideoPlanDocument`: title, objective, exact duration,
   scenes, selected artifacts, visual direction, and remote prompt.
4. Apply semantic RFC 6902 plan review unless `--no-review` is set.
5. Branch to engine-specific authoring/rendering.
6. On source/renderer validation failure, perform at most one focused source
   repair. With review enabled, use the reviewer; otherwise use the code author.
7. Inspect the final MP4 with `ffprobe` for container, codec, dimensions,
   duration, frame expectations, and audio policy.
8. Commit one immutable revision and publish only the MP4 when requested.

`--no-review` disables semantic plan review and the one technical source-repair
attempt. Deterministic source validation, snapshots, and final media inspection
remain mandatory.

## Model Resolution

| Stage | Required selection |
| --- | --- |
| Plan drafter | `text` profile. |
| Semantic review | Reviewer role with text capability. |
| Hyperframes/Manim author | `code` profile; falls back to drafter only when the drafter declares `code`. |
| Focused local source repair | Reviewer when enabled, otherwise code author. |
| Automated frame review | Reviewer role, only when its profile declares `image`. |
| Remote video | `video` profile. |
| Optional reference generation | `image` profile when `image-gen` is enabled. |

For local engines, `--model video=...` is invalid. For the remote model engine,
`--model code=...`, `--fps`, and `--quality` are invalid.

## Shared Options

| Option | Behavior |
| --- | --- |
| `--duration` | Required, 1 through 3600 seconds at core validation; remote catalog may impose tighter limits. Hyperframe authors one scene per request, so longer films are viable, but a plan with very many short scenes costs one model call each. |
| `--resolution` | Local accepted values are `480p`, `720p`, `1080p`; defaults to `1080p` locally and `720p` remotely. |
| `--aspect-ratio` | Positive `W:H`, default `16:9`. Local width is derived and rounded to an even pixel count. |
| `--fps` | Local only, 1 through 120, default 30. |
| `--quality` | Local only: `draft`, `standard`, or `high`; default `high`. |
| `--audio` | Local engines require `off`; remote default is `auto`. |
| `--out` | Publishes MP4 under `<out>/_sfumato/videos`. |
| `--allow-code-execution` | Manim only; rejected for other engines. |

## Hyperframes Engine

### Authoring, scene by scene

Sfumato generates the entry composition itself — the canvas, one mount per planned
scene, and a timeline that runs for the requested duration — and asks the model for
one scene composition per request. Every part of that entry file is a contract the
renderer enforces, so generating it removes a whole class of authoring failure, and
each request carries a single beat, its own time window, and how the previous beat
leaves the frame.

That last part is what makes the seam rule actionable: a scene knows what it is
entering from, so a boundary can read as one continuous move instead of two
animations meeting.

The practical consequence for duration is worth stating plainly. The hard limit is
3600 seconds, but before this change one model response had to carry the whole film,
so anything past a minute or two degraded regardless of the limit. Per-scene
authoring is what makes minutes viable.

### Empty-frame gate

Snapshots were always captured at every scene start and midpoint and written to a
contact sheet. They are now measured too, without spending a model: Sfumato reads
each frame's ink coverage and colour count, using the frame's own dominant colour as
the background so it works for any theme.

A scene that opens on an empty frame is a defect, because the cut lands on nothing
and reads as a stutter. Away from a scene boundary only a completely blank frame
counts, since holding on one word over a plain ground is a real choice. A defect
triggers one focused source repair, the film is re-measured, and anything that
survives is reported in the warnings and in `review.frame_defects`.

This was not hypothetical: a real 45-second film had all four of its scene
boundaries land on empty frames, one of them completely blank.

### Renderer-safe fonts

The renderer resolves every family in a stack, fallbacks included, and fails the
render on any it cannot supply. A real film was lost to `font-family: JetBrains
Mono, Fira Code, monospace`, where only the unused fallback was unbundled. Sfumato
now rejects an authored scene that names such a family before the renderer ever runs,
which routes it back through the scene author instead of failing the whole film.

### Automated frame review

When the reviewer profile declares the `image` capability, Sfumato attaches the
rendered frames to a review request and asks what they actually look like. This is
the layer that catches what counting pixels cannot: clipped text, overlapping
elements, and frames that are technically populated but unreadable. The measurements
travel with the frames so the model judges composition rather than re-deriving
coverage.

The review is advisory. Its findings appear in the warnings and in `visual_report`,
and `visual_review_mode` reports `automated`. A text-only reviewer leaves the mode at
`evidence_only`: the frames and the deterministic verdict still ship, and Sfumato
does not claim an inspection that never happened. A model that declares it cannot
read images refuses the request rather than answering from the prompt alone.

Frames travel as file paths, not bytes. The Codex App Server takes a local path and
opens the file itself, while the HTTP connectors inline base64, so a path serves both
and spares the caller an encode that one of them discards.

### Production pipeline

Hyperframe generation is a silent production pipeline: Sfumato derives a workflow,
records `DESIGN.md`, `STORYBOARD.md`, and `SCRIPT.md` (on-screen copy only), authors
scene direction, validates the source, and captures deterministic review snapshots
before the final encode. Audio, TTS, music, SFX, and audio-timed captions are not
part of this engine.

Use `--workflow auto` (the default) to select a route from the brief, or select one
of `explainer`, `motion-graphics`, `product-launch`, `talking-head`, `slideshow`, or
`general`. `--url <URL>` records an explicit website source for a Hyperframe brief.

The managed renderer installs a pinned local catalog of approved Hyperframe blocks
and effects. Generation never downloads catalog items; `sfumato renderer doctor
hyperframe` reports a missing or incompatible catalog.

The planner, the semantic reviewer, the source author, and the source-repair pass
all see that installed catalog, so every stage that can decide a selection judges
it against the same list. Registry items ship as standalone documents that load
GSAP from a CDN and fonts from Google; Sfumato rewrites each one as it stages it
into the render workspace, pointing it at the pinned local GSAP and dropping remote
font requests, and fails the render if any remote reference survives. Affected
items fall back to the theme's fonts.

Registry items that load their own content at render time are not curated. The two
caption styles are the current example: they read words with per-word timings from
a sidecar file, which only a narration track can supply, and this engine is silent.
They return to the catalog once the engine renders audio; until then, selecting an
item like that fails at staging with that reason rather than mid-render.

### Catalog items are references, not components

A selected item reaches the scene author as source to adapt, not as a path to mount.
This is measured, not stylistic: the registry files are showcase documents. None of
them carries a `<template>`, and their copy demonstrates the technique on unrelated
content — `flowchart` asks "Should I learn to code?", `whip-pan` labels its halves
"SCENE A" and "SCENE B", `data-chart` charts "Monthly Revenue".

Mounting one therefore puts that demonstration on screen. A real film about fibre
optics mounted the flowchart, and the author — told never to edit a mounted block —
covered the foreign copy with its own ground, which the renderer then rejected as
hidden text, repair after repair. Handing over the technique instead keeps what is
valuable about the catalog and leaves the words to the film.

For a human checkpoint before rendering, pass `--visual-review`. Sfumato stores the
source bundle, snapshots, and `contact-sheet.md` under its managed review session,
then reports the `review_id`. Preview and approval commands operate on that exact
session; a normal generation remains non-interactive and renders after validation.

```bash
sfumato generate video --engine hyperframe --duration 12 --audio off \
  --url https://example.com/product --workflow product-launch \
  --visual-review --instruction "Present the product dashboard in three beats"
sfumato video preview <review-id>
sfumato video approve <review-id>
```

The review session remembers the effective `--out` destination from generation.
After approval, Sfumato publishes the MP4 under that destination's
`_sfumato/videos/<slug>/` directory. Pass `--out <folder>` to `video approve` only
when you intentionally want to override the destination saved with the review:

```bash
sfumato video approve <review-id> --out ./course-materials
```

Install and check the explicit managed renderer:

```bash
sfumato renderer install hyperframe
sfumato renderer doctor hyperframe
```

Generation example:

```bash
sfumato generate video \
  --project university \
  --engine hyperframe \
  --duration 10 \
  --resolution 720p \
  --aspect-ratio 16:9 \
  --fps 30 \
  --quality draft \
  --audio off \
  --model text=codex \
  --model code=codex \
  --instruction "Animate the decomposition of a square wave"
```

The author returns strict JSON containing `meta.json` and `index.html`.
Sfumato's contract requires:

- a root composition `<div>` with stable ID, composition ID, start, width, and height;
- timed clips with IDs, `class="clip"`, start, duration, and track index;
- local `./vendor/gsap.min.js` loaded from the managed pinned runtime;
- one paused finite GSAP timeline;
- timeline registration in `window.__timelines` under the exact composition ID;
- no network, modules, filesystem escapes, narration, or unseeded randomness.

Sfumato runs Hyperframes `lint`, `check`, and `render`. Actual lint/render output
is returned to the source-repair model when the first source is invalid.

Hyperframes videos are silent in this milestone. Optional transcription, TTS,
MusicGen, and Docker checks do not block health.

## Manim Engine

Install and check:

```bash
sfumato renderer install manim
sfumato renderer doctor manim
```

Generate with explicit authorization:

```bash
sfumato generate video \
  --project university \
  --engine manim \
  --duration 15 \
  --resolution 1080p \
  --fps 30 \
  --audio off \
  --allow-code-execution \
  --model code=codex \
  --instruction "Animate Fourier synthesis geometrically"
```

The author returns strict JSON containing `scene.py` and must define
`SfumatoScene`. Sfumato rejects dangerous imports and operations including OS,
subprocess, sockets, HTTP clients, dynamic execution, and arbitrary file opens.
It then compiles Python syntax and runs the managed Manim environment.

Authorization can persist in project config:

```bash
sfumato config set security.allow_manim true \
  --scope project --project university
```

This permission does not make generated Python a strong sandbox. Inspect trust
requirements before enabling it persistently.

## Remote Model Engine

Configure a video-capable OpenRouter profile:

```bash
sfumato model add remote-video \
  --connector openrouter \
  --id provider/video-model \
  --capability video \
  --option video_timeout_seconds=900
sfumato model use video remote-video --project university
```

Generate:

```bash
sfumato generate video \
  --project university \
  --engine model \
  --duration 8 \
  --resolution 720p \
  --aspect-ratio 16:9 \
  --audio auto \
  --model video=remote-video \
  --instruction "Visualize harmonic synthesis as a clean educational animation"
```

The OpenRouter adapter:

1. queries the provider video model catalog;
2. validates duration, resolution, ratio, audio, and reference support before a paid request;
3. submits the asynchronous video job;
4. polls at the profile interval until completion or timeout;
5. downloads the first MP4 result;
6. passes it through the same local `ffprobe` inspection.

Selected local artifacts are sent as Base64 references only when the catalog
announces reference support. One request produces one clip; there is no stitching.

## Image References

Video planning may use `image-gen` to create visual references for local or
remote generation. Enable it persistently or per request:

```bash
sfumato tool enable image-gen --project university
sfumato generate video --engine hyperframe --duration 10 \
  --instruction "..." --tool image-gen
```

The planner sees project artifacts and generated images with theme-aware paths.
Only selected/referenced assets survive the transaction.

## Artifacts

Managed revision:

```text
~/.sfumato/Projects/<project>/resources/videos/<resource-id>/
├── current.json
└── revisions/<revision-id>/
    ├── manifest.json
    ├── <slug>.mp4
    ├── plan.json
    ├── storyboard.md
    ├── source/
    │   ├── meta.json and index.html   # Hyperframes
    │   └── scene.py                   # Manim
    └── assets/
```

Publication:

```text
<out>/_sfumato/videos/<slug>/<slug>.mp4
```

Source, plan, storyboard, and revision history remain managed and are not copied
to the publication directory.

## Dry Run And JSON

Dry run resolves both authoring profiles, selected artifacts/tools, theme,
prompts, engine settings, and planned MP4 path. It does not call a model,
renderer, remote video API, inspector, or artifact store.

JSON output includes project, generated title, engine, committed MP4 path,
models by role/capability, tools, project assets, every committed file,
published MP4, semantic/source/inspection statuses, prompt provenance, and warnings.
