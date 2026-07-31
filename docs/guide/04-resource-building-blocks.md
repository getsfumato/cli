# Resource Building Blocks

Sfumato composes generation from several independently managed concepts. A
theme is not a template, a page plugin is not a model-facing tool, and a local
renderer is not a remote video model.

## Themes

A project must select exactly one theme. Themes are global reusable packages:

```text
<platform-config>/sfumato/themes/<theme-name>/
├── theme.toml
├── marp/theme.css
└── html/
    ├── page.html
    ├── style.css
    └── script.js
```

`theme.toml` contains semantic colors and fonts plus relative adapter paths.
The Marp stylesheet must declare Marpit theme metadata. The HTML shell must
contain exactly one `<!-- SFUMATO_CONTENT -->` marker. Adapter paths must be
relative and cannot traverse outside the package.

### Theme Commands

```bash
sfumato theme create <name>
sfumato theme import <DESIGN.md> [--name <name>]
sfumato theme export <name> [--out <path>]
sfumato theme list
sfumato theme show <name>
sfumato theme use <name> [--project <project>]
```

- `create` copies the bundled `sfumato-default` scaffold and rewrites its name.
- Duplicate or invalid portable names are rejected.
- `import` consumes a Google DESIGN.md document, validates normative color and
  typography tokens, preserves source guidance, and creates Marp/HTML adapters.
- `export` writes a self-contained DESIGN.md; output defaults to `DESIGN.md`.
- `show` prints the manifest and adapter information.
- `use` writes the selected theme into the active or explicit project.
- Generation may temporarily override the project theme with `--theme`.
- Missing themes or missing renderer adapters fail; there is no silent fallback.

Examples:

```bash
sfumato theme create gruvbox
sfumato theme use gruvbox --project university
sfumato theme export gruvbox --out ./DESIGN.md
sfumato theme import ./ferrari-DESIGN.md --name ferrari
```

## Structural Templates

Templates are optional resource structures. They are never applied implicitly.
The generator uses one only when `--template <name>` is present.

```text
<platform-config>/sfumato/templates/<name>/
├── template.toml
└── <structural-source>
```

The structural source must contain exactly one:

```html
<!-- SFUMATO_CONTENT -->
```

Supported kinds are `slides` and `page`.

### Template Commands

```bash
sfumato template create <name> --kind slides|page [--from <path>]
sfumato template list [--kind slides|page]
sfumato template show <name> --kind slides|page
```

- Without `--from`, `create` generates a valid scaffold.
- With `--from`, the source is copied after validating its marker and kind.
- `list` may filter by kind.
- `show` prints metadata and complete structural source.
- During generation, the drafter returns only content for the marker; Sfumato
  performs the structural merge before validation and review.

## Project Artifacts

A project artifact is a reusable logical visual such as a logo, chart, icon, or
diagram. It has semantic metadata and one or more theme variants.

Project artifacts live under:

```text
<project-root>/.sfumato/assets/
```

Supported files are PNG, JPEG, WebP, GIF, and passive SVG. SVG validation rejects
scripts and unsafe external behavior.

Variant selection prefers the exact active theme and then the wildcard `*`
variant. A generation recipe lets an image model reconstruct a missing theme
variant. Only artifacts referenced by the final reviewed resource are copied
into its immutable revision.

### Add

```bash
sfumato artifact add <path> \
  [--name <name>] \
  [--description <text>] \
  [--alt-text <text>] \
  [--tag <tag>]... \
  [--prompt <image-generation-recipe>] \
  [--theme <theme> | --all-themes] \
  [--project <project>]
```

- `--name` defaults from the source filename when omitted.
- `--theme` registers an exact-theme variant.
- `--all-themes` registers the wildcard variant and conflicts with `--theme`.
- `--prompt` describes how to regenerate equivalent content for another theme.
- The original source remains untouched; Sfumato copies and hashes the file.

Example:

```bash
sfumato artifact add ./figures/spectrum.png \
  --name square-wave-spectrum \
  --description "Odd-harmonic square-wave spectrum" \
  --alt-text "Amplitude bars at odd harmonic frequencies" \
  --tag fourier --tag spectrum \
  --prompt "Recreate the labeled spectrum with the same data" \
  --theme gruvbox \
  --project university
```

### Edit

```bash
sfumato artifact edit <name> \
  [--description <text>] \
  [--alt-text <text>] \
  [--tag <tag>]... \
  [--prompt <recipe> | --clear-prompt] \
  [--from-theme <theme> --to-theme <theme>] \
  [--project <project>]
```

- Repeated `--tag` values replace all tags when at least one is supplied.
- `--clear-prompt` removes the regeneration recipe.
- `--from-theme` and `--to-theme` must be passed together and reassign a variant.

### Inspect And Remove

```bash
sfumato artifact list [--project <project>]
sfumato artifact show <name> [--project <project>]
sfumato artifact remove <name> [--project <project>]
```

`show` reports metadata, media type, theme variants, content hashes, and copied
paths. `remove` deletes the project catalog entry and its managed copies; it
does not delete the original source file used by `artifact add`.

## Prompt Templates

All model-facing language is stored as MiniJinja Markdown templates. Rust owns
typed policy, validators, retry counts, and output contracts; prompt files own
the prose presented to models.

Each prompt ID resolves independently in this order:

1. `<project-root>/.sfumato/prompts/`;
2. the user prompt directory under platform configuration;
3. bundled adapter templates.

An existing invalid override stops the operation before provider invocation.
Sfumato never silently falls back past an invalid file. Includes are restricted
to allowed roots and reject absolute paths, traversal, and cycles.

### Prompt Commands

```bash
sfumato prompt list [--project <project>]
sfumato prompt show <id> [--project <project>]
sfumato prompt customize <id> --scope user|project [--project <project>]
sfumato prompt validate [--project <project>]
```

- `list` is the authoritative source of valid stable prompt IDs.
- `show` prints the resolved source, origin, version, and content hash.
- `customize` copies the bundled source to the chosen override directory and
  does not launch an editor.
- Project scope resolves the active project unless `--project` is supplied.
- `validate` strictly renders every resolved template with fixture values and
  reports its origin/hash.
- Artifact manifests record every prompt ID, origin, version, and SHA-256 hash
  used by generation.

Recommended customization workflow:

```bash
sfumato prompt list --project university
sfumato prompt show slides.draft.user --project university
sfumato prompt customize slides.draft.user --scope project --project university
# Edit the created Markdown template.
sfumato prompt validate --project university
```

## Page Plugins

Page plugins are browser assets installed from pinned public-CDN recipes. The
installer verifies SHA-256 hashes and licenses, then stores offline packages
under `~/.sfumato/plugins`. Generation never runs npm or downloads a plugin.

Categories are deliberately distinct:

| Category | Selection rule | Initial IDs |
| --- | --- | --- |
| UI | Exactly zero or one active per page/project. Enabling another replaces the previous UI. | `shadcn`, `materialui` |
| Utility | Any number may be combined. | `motion`, `threejs`, `theatre`, `lottie` |
| Runtime | Internal transitive dependency; cannot be enabled directly. | `react`, `react-dom` |

Current pinned public releases include Motion 12.42.2, Three.js 0.184.0,
Theatre.js Core 0.7.2, Lottie Web 5.13.0, Material UI 5.15.14, React 18.3.1,
and the catalog-pinned Shadcn New York v4 source definitions.

### Plugin Commands

```bash
sfumato plugin list [--project <project>]
sfumato plugin show <id>
sfumato plugin install <id> [--version <version>]
sfumato plugin update <id>
sfumato plugin enable <id> [--project <project>]
sfumato plugin disable <id> [--project <project>]
```

- `list` reports catalog version, installed version, and project enablement.
- `show` reports category, API global, model guidance, hash, and license when installed.
- `install` downloads only during this explicit command and installs dependencies first.
- `update` installs the catalog's current pinned release.
- `enable` updates project page defaults. A UI replaces the current UI;
  utilities append/deduplicate.
- `disable` clears that UI or removes that utility from the selected project.

Page request overrides use `--ui`, `--plugin`, and `--disable-plugin`; see
[Generate pages](06-generate-pages.md).

## Generation Tools

Generation tools are provider-backed functions exposed to a drafter. They are
not page JavaScript plugins.

```bash
sfumato tool list [--project <project>]
sfumato tool enable image-gen|video-gen|audio-gen [--project <project>]
sfumato tool disable image-gen|video-gen|audio-gen [--project <project>]
```

`tool list` reports whether the project enables each tool and whether a required
model profile is configured.

Compatibility matrix:

| Resource | `image-gen` | `video-gen` | `audio-gen` |
| --- | --- | --- | --- |
| Slides | Yes | No | No |
| Pages | Yes | Yes, remote model only and at most once per page generation. | Yes, as `sfumato_audio_gen`. |
| Videos | Yes during planning/reference selection | No | Yes, but not as a tool: enabling it makes a Hyperframe film narrated, and Sfumato speaks the plan's per-scene lines itself. |

Filesystem list/read tools are internal, read-only, mandatory, and restricted
to the selected project/source roots. They do not appear in project tool config.
Directory listings are sorted and capped at 200 entries with a `truncated`
marker. A direct model `read_file` call is limited to 128 KiB and rejects files
that exceed the limit instead of silently abbreviating them.

Generated tool responses are also bounded before writing: images may be at most
64 MiB and page videos at most 512 MiB. Generated filenames are content-addressed,
and unreferenced generated media is removed before the final artifact commit.

One-request overrides:

```bash
sfumato generate page --instruction "..." --tool image-gen --tool video-gen
sfumato generate slides --instruction "..." --disable-tool image-gen
```

## Local Video Renderers

Renderers are explicitly installed process environments. They are not model
profiles and Sfumato never falls back between them.

```bash
sfumato renderer list
sfumato renderer install hyperframe|manim
sfumato renderer remove hyperframe|manim
sfumato renderer doctor [hyperframe|manim]
```

- `list` reports pinned version, installation, health, and dependency details.
- `install` is the only operation allowed to invoke npm/uv and download packages.
- `remove` deletes the managed renderer installation, not generated videos.
- `doctor` checks one renderer or both when omitted.

### Hyperframes

The managed installation includes pinned Hyperframes and a pinned local GSAP
runtime. Generation copies GSAP into the source revision so rendering does not
depend on a CDN. Required health checks are Node.js, FFmpeg, FFprobe, Chrome,
the managed executable, and the managed GSAP runtime. Transcription, local TTS,
MusicGen, and Docker are optional for Sfumato's initial silent workflow.

```bash
sfumato renderer install hyperframe
sfumato renderer doctor hyperframe
```

### Manim

Sfumato creates a managed Python environment through `uv`, installs the pinned
Manim package, validates generated Python, and invokes Manim through that
environment. FFmpeg and FFprobe remain external dependencies.

```bash
sfumato renderer install manim
sfumato renderer doctor manim
```

Manim executes generated Python. Generation additionally requires project
`security.allow_manim = true` or the per-command `--allow-code-execution` flag.
This is explicit authorization, not a strong sandbox.
