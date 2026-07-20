# Generate Videos

## Command

```bash
sfumato generate video [INPUTS]... \
  --instruction <text> \
  --engine hyperframe|manim|model \
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

`--no-review` disables semantic plan review. It does not disable deterministic
source validation or the one recovery attempt needed to turn rejected generated
source into a renderable project.

## Model Resolution

| Stage | Required selection |
| --- | --- |
| Plan drafter | `text` profile. |
| Semantic review | Reviewer role with text capability. |
| Hyperframes/Manim author | `code` profile; falls back to drafter only when the drafter declares `code`. |
| Focused local source repair | Reviewer when enabled, otherwise code author. |
| Remote video | `video` profile. |
| Optional reference generation | `image` profile when `image-gen` is enabled. |

For local engines, `--model video=...` is invalid. For the remote model engine,
`--model code=...`, `--fps`, and `--quality` are invalid.

## Shared Options

| Option | Behavior |
| --- | --- |
| `--duration` | Required, 1 through 3600 seconds at core validation; remote catalog may impose tighter limits. |
| `--resolution` | Local accepted values are `480p`, `720p`, `1080p`; defaults to `1080p` locally and `720p` remotely. |
| `--aspect-ratio` | Positive `W:H`, default `16:9`. Local width is derived and rounded to an even pixel count. |
| `--fps` | Local only, 1 through 120, default 30. |
| `--quality` | Local only: `draft`, `standard`, or `high`; default `high`. |
| `--audio` | Local engines require `off`; remote default is `auto`. |
| `--out` | Publishes MP4 under `<out>/_sfumato/videos`. |
| `--allow-code-execution` | Manim only; rejected for other engines. |

## Hyperframes Engine

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
