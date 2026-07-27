# Troubleshooting

Use this chapter by symptom. Run diagnostic commands before changing stored
state, and preserve the exact error `code`, `class`, `stage`, and message when
reporting failures.

## Hyperframe production reviews

Run `sfumato renderer doctor hyperframe` before investigating an authoring failure.
It verifies the pinned CLI, GSAP runtime, and the managed local catalog. A generated
Hyperframe source must keep one paused timeline per composition, use only local
assets, and never fetch resources at render time.

If a scene is blank or visually weak, inspect the managed review session's
`contact-sheet.md` and `snapshots/` before regenerating. Preview/render differences
usually come from unavailable fonts or a version mismatch; reproduce with the stored
source bundle and report the renderer version, `check` output, and contact sheet.

This release does not diagnose audio because Hyperframe output is intentionally silent.

## Fast Diagnostic Checklist

```bash
sfumato project list
sfumato project show
sfumato config show --scope effective
sfumato connector list
sfumato model list
sfumato theme list
sfumato prompt validate
```

For pages:

```bash
sfumato plugin list
sfumato tool list
```

For local video:

```bash
sfumato renderer list
sfumato renderer doctor hyperframe
sfumato renderer doctor manim
```

Then run the failing generation with the same arguments plus `--dry-run`. A dry
run proves resolution and prompt rendering, not provider connectivity or native
renderer health.

## Binary And Installation

### `sfumato: command not found`

Build the root package and install or link the executable:

```bash
cargo build -p sfumato
cargo install --path . --locked
```

For development, a symlink may point to the debug binary:

```bash
ln -sf "$(pwd)/target/debug/sfumato" "$HOME/.local/bin/sfumato"
```

Ensure `$HOME/.local/bin` is in `PATH`. A symlink does not rebuild the binary;
run `cargo build -p sfumato` after source changes.

### The CLI appears older than the source

Compare the resolved executable and rebuild:

```bash
command -v sfumato
ls -l "$(command -v sfumato)"
cargo build -p sfumato
sfumato --help
```

An installed release and `target/debug/sfumato` are separate files.

## Configuration And Schema

### Legacy configuration is rejected

Schema v5 is current. Unsupported legacy/future schemas are rejected instead of
being rewritten during reads. The one supported automatic upgrade is v4 to v5;
it writes a backup and atomically installs the validated replacement.

If the file uses the much older singular `[inference]`/`[providers]` shape,
recreate it intentionally:

```bash
sfumato init user --force
```

This replaces global Sfumato configuration; record any connector/model choices
you need first. It does not delete project source files or managed artifacts.

### Locate configuration files

Platform configuration uses the operating system's config directory:

- macOS: `~/Library/Application Support/sfumato/`
- Linux: usually `~/.config/sfumato/`

Global configuration and project registry are stored there. Portable project
configuration is always `<project-root>/.sfumato/project.toml`. Managed resource
revisions live separately under `~/.sfumato/Projects/`.

Use commands rather than guessing paths:

```bash
sfumato config show --scope user
sfumato project show university
sfumato config show --scope effective --project university
```

### `config set` or `config delete` fails validation

Each operation validates the complete resulting document. Common causes:

- deleting a required project theme;
- assigning an unknown model profile;
- setting a model as a default for an unsupported capability;
- selecting an uninstalled page plugin;
- malformed TOML arrays or booleans;
- attempting to write the read-only `effective` scope;
- attempting raw secret editing.

Inspect the current scope, then change the owning object with its dedicated
command where possible:

```bash
sfumato model use text codex --project university
sfumato theme use gruvbox --project university
sfumato plugin enable motion --project university
sfumato tool disable video-gen --project university
```

### Active project is wrong

```bash
sfumato project list
sfumato project use university
```

For automation, pass `--project university` instead of relying on global active
state.

### Project path no longer exists

Removing a project from the registry does not delete files. If the root moved,
remove the stale registration and initialize it at the new location:

```bash
sfumato project remove university
sfumato init project university --path "/new/vault/path"
```

Do not use this sequence until the new path and existing `.sfumato/project.toml`
have been inspected.

## Connector Authentication And Connectivity

### OpenRouter credential is unavailable

Inspect connector configuration and its secret reference:

```bash
sfumato connector show openrouter
sfumato connector auth-status openrouter
```

For a developer workstation, save the key in the OS keyring:

```bash
sfumato connector login openrouter
```

For CI, configure an environment reference and export it in the same process
environment that launches Sfumato:

```bash
sfumato connector setup openrouter --api-key-env OPENROUTER_API_KEY
export OPENROUTER_API_KEY='...'
sfumato connector auth-status openrouter
```

Never pass the key as a command-line argument or store it as a raw TOML value.

### Slides worked but another shell says the key is missing

The successful process may have inherited an environment variable that the new
terminal, IDE, launch agent, or TUI process does not inherit. Alternatively, the
connector may now point to a different named profile. Compare:

```bash
sfumato connector show openrouter
sfumato connector auth-status openrouter
sfumato config show defaults
sfumato config show --scope effective --project university
```

Using OS-keyring storage avoids environment inheritance for local CLI usage.

### Codex authentication is unavailable

Sfumato does not own Codex credentials. Check the Codex CLI directly:

```bash
codex login status
sfumato connector auth-status codex
sfumato connector status codex
```

Run `codex login` if required. Connector `login`/`logout` are not the correct
commands for Codex.

### Ollama connection fails

Confirm the daemon and native catalog:

```bash
ollama list
sfumato connector status ollama
sfumato connector models ollama
```

The default OpenAI-compatible endpoint is `http://localhost:11434/v1`. The
profile's provider model ID must exactly match an installed Ollama tag.

### HTTP 400: invalid model ID

Provider model IDs are exact and may change independently of Sfumato. Query the
native catalog and update the profile rather than guessing prefixes:

```bash
sfumato connector models openrouter
sfumato model edit grok-latest --id <exact-catalog-id>
sfumato model show grok-latest
```

### Connector supports the endpoint but generation says capability missing

Connector support and model profile capability declarations are distinct.
Inspect and edit the profile:

```bash
sfumato model show my-profile
sfumato model edit my-profile \
  --capability text \
  --capability code
```

Supplying any capability flags replaces the complete capability set, so include
every capability the profile should retain.

## Model Resolution

### No model is configured for a stage

Inspect effective defaults and profiles:

```bash
sfumato config show --scope effective --project university
sfumato model list
```

Assign required defaults:

```bash
sfumato model use text codex --project university
sfumato model use reviewer grok-latest --project university
sfumato model use image gpt-image --project university
sfumato model use video remote-video --project university
```

Local Hyperframe/Manim authoring also needs `code`. If the resolved text
profile declares `code`, it can be the fallback; otherwise assign a code
default or pass `--model code=<profile>`.

### A model cannot be removed

The profile is referenced by a global/project capability default or reviewer
role. Find likely references:

```bash
sfumato config show --scope user
sfumato project list
sfumato config show --scope project --project university
```

Reassign each reference with `model use`, then remove it.

### Reviewer returns too little content

Reviewers do not regenerate complete slide/page resources. They return
constrained RFC 6902 patches. If review fails with invalid JSON or an unsupported
path, Sfumato can preserve the prior valid candidate and report a warning.

Check reviewer capability/options:

```bash
sfumato model show reviewer-profile
sfumato prompt show slides.review.user --project university
sfumato prompt validate --project university
```

Increase `max_tokens` only when the patch itself is legitimately truncated; it
is not a substitute for a valid patch contract.

### Tool round limit is exceeded

The model repeatedly called filesystem or generation tools without returning
the required final contract. Increase the text profile's `max_tool_rounds` only
after checking whether the instruction/source root causes unnecessary browsing:

```bash
sfumato model edit codex --option max_tool_rounds=10
```

Prefer explicit input paths and a focused instruction. Sfumato uses a dedicated
tool-exhaustion prompt for the final response rather than allowing unbounded
rounds.

## Sources And Project Instructions

### Source path does not exist

Sfumato canonicalizes all requested inputs. Shell quoting errors are common for
paths with spaces or apostrophes:

```bash
sfumato generate slides \
  "/Users/alex/Documents/Alex's Notebook/Facultad/Análisis Numérico" \
  --instruction "Explain the selected notes"
```

Do not duplicate a path segment when combining project root and relative paths.

### Expected files were ignored

Initial source extensions are:

```text
.md .txt .rs .py .js .ts .html .css .json .toml .yaml .yml
```

Unsupported binary/media files are not ingested as textual sources. Register
reusable images with `artifact add`. Current source safety budgets are 256
files, 1 MiB per file, and 16 MiB aggregate. `SFUMATO.md` has a 64 KiB limit.

Unreadable or over-budget files are reported; reduce the source set or split
material deliberately.

### `SFUMATO.md` was not applied

The file must be named exactly `SFUMATO.md` in the registered project working
root, not the process's arbitrary current directory:

```bash
sfumato project show university
sfumato generate slides --project university --instruction "..." --dry-run
```

Dry-run human output reports whether project instructions were found.

## Themes, Templates, And Artifacts

### Unknown or incomplete theme

```bash
sfumato theme list
sfumato theme show gruvbox
sfumato theme use gruvbox --project university
```

Slides require the Marp CSS adapter. Pages require the HTML shell/CSS adapter.
Adapter paths must be relative and remain inside the package. There is no silent
fallback to the default theme.

### Theme CSS appears as slide content

The generated deck should reference copied custom theme CSS through Marp
rendering; it should not contain the complete stylesheet as ordinary Markdown.
Inspect the committed `deck.md` frontmatter and `themes/<theme>.css`. Validate
that model output contains one proper Marp frontmatter block at the beginning.

Sfumato normalization removes known noisy wrappers, but malformed model output
may still fail validation and should be reported with its prompt provenance.

### Template was used unexpectedly

Templates are opt-in. New commands use no template unless `--template <name>`
is present. Check the actual command/TUI selection and JSON `template` field.

The deprecated page `--shadcn` flag selects a UI library; it is not a structural
template.

### Template marker error

Template source requires exactly one literal marker:

```html
<!-- SFUMATO_CONTENT -->
```

Use `template show` to inspect the stored source. Do not use a Markdown code
fence containing a second example marker.

### Artifact is not offered or copied

Check project, active theme, and variants:

```bash
sfumato artifact show square-wave-spectrum --project university
sfumato theme show ferrari
sfumato generate page --project university --instruction "..." --dry-run
```

Selection prefers the exact theme, then wildcard `*`. A missing variant can be
regenerated only when the artifact has a recipe and an image model is available.
Only artifacts referenced by final reviewed output are committed.

## Prompt Overrides

### Generation fails before contacting a model

An invalid existing prompt override intentionally blocks fallback. Validate and
inspect origin/hash:

```bash
sfumato prompt validate --project university
sfumato prompt list --project university
sfumato prompt show <id> --project university
```

Typical failures are unknown strict variables, unsafe absolute/traversing
includes, include cycles, or malformed MiniJinja syntax. Fix the reported
override file; do not delete it unless you explicitly want to return to the
next precedence layer.

### Prompt changes do not appear

Resolution is per prompt ID: project override, then user override, then bundled.
A project override shadows the user copy for that ID only. Use `prompt show` to
confirm the resolved origin and SHA-256 hash.

## Page Plugins And Tools

### Plugin is known but unavailable during generation

Catalog support is not installation. Explicitly install, then enable or pass it:

```bash
sfumato plugin show motion
sfumato plugin install motion
sfumato plugin enable motion --project university
sfumato plugin list --project university
```

Generation is offline and never installs missing runtimes automatically.

### Shadcn and Material UI conflict

They are both exclusive UI libraries. Enabling one replaces the other. Use
utilities independently:

```bash
sfumato plugin enable shadcn --project university
sfumato generate page --ui shadcn --plugin motion --instruction "..."
```

Do not pass UI IDs through `--plugin`; use `--ui`. Use `--ui none` to disable
the project UI for one request.

### `image-gen` or `video-gen` does not appear

Inspect both persisted tool state and model defaults:

```bash
sfumato tool list --project university
sfumato config show --scope effective --project university
sfumato model list
```

`image-gen` requires an image profile. `video-gen` requires a video profile and
is only exposed to page drafting, with one invocation maximum. Standalone video
planning supports image generation but never recursively exposes video-gen.

Command overrides are explicit:

```bash
sfumato generate page --instruction "..." --tool image-gen
sfumato generate page --instruction "..." --disable-tool video-gen
```

## Slide Rendering

### Marp CLI is missing

Install the official CLI and verify it is in the process `PATH`:

```bash
npm install -g @marp-team/marp-cli
marp --version
```

When PDF cannot be rendered, a valid managed Markdown revision may still be
committed with a warning depending on the workflow. Sfumato must not preserve a
stale PDF as if it belonged to the new revision.

### Marp exits with status 1

Run the exact command conceptually reproduced by Sfumato against the committed
deck/theme to isolate Marp:

```bash
marp --theme /absolute/path/to/themes/theme.css \
  /absolute/path/to/deck.md \
  -o /tmp/deck.pdf
```

Inspect stderr for invalid frontmatter, CSS, browser, local-file, or diagram
problems. Sfumato does not enable Marp's broad `--allow-local-files` switch.

### Browser executable is unavailable

Sfumato dynamically discovers supported Chrome/Chromium/Edge installations; it
does not use one hardcoded user path. Ensure a supported browser is installed
and visible to the process. Browser absence can skip optional layout/page
inspection with a warning, but Marp or Hyperframes may still require a browser
for rendering.

### Mermaid CLI is missing

```bash
npm install -g @mermaid-js/mermaid-cli
mmdc --version
```

Mermaid rendering is local; there is no Kroki/network fallback. Sfumato passes
theme-derived variables and requests transparent diagram backgrounds before
embedding SVG output.

### Mermaid parse error or unclosed fence

The workflow validates fenced diagrams and may send the exact renderer error to
one focused repair request. If the repaired source remains invalid, generation
reports the failing slide/diagram rather than silently embedding broken syntax.

Inspect the Markdown around the named slide. Mermaid content cannot contain a
slide separator accidentally appended inside an open fence.

### Math appears as raw LaTeX

Slides use Marp `math: mathjax` with `$...$` for inline math and `$$...$$` for
display math. Page resources are different: their offline assembler expects
`\(...\)` and `\[...\]`. Do not copy the page convention into Marp Markdown.

Current slide prompts state this distinction explicitly. Before review and
rendering, Sfumato also converts paired classic delimiters in slide prose to
dollar delimiters while leaving code fences, inline code, and Mermaid untouched.
Older binaries do not contain that normalization, so rebuild before retrying.

Check that the model did not place equations inside an ordinary code fence.

### Image or content overflows a slide

Sfumato renders a temporary themed preview at 1280x720, measures horizontal and
vertical overflow, and performs focused slide repair when review is enabled.
Generated image CSS also constrains fitting. Remaining issues are returned by
slide number without discarding an otherwise valid deck.

Improve the instruction, source selection, or theme CSS when the reviewer
cannot safely reduce the slide. Prefer splitting content to globally shrinking
fonts.

## Page Rendering

### Page has an unwanted outer frame or width constraint

The final page combines three sources:

1. theme HTML shell/CSS;
2. optional structural template;
3. model-generated `body_html`, CSS, and JavaScript.

Inspect the theme adapter first with `theme show`, then the committed HTML for
generated selectors such as `max-width`, borders, or centered containers. If
the frame is theme-owned, edit the theme HTML/CSS. If it appears only in one
resource, use focused prompt customization or regenerate with an explicit
layout instruction.

### Page works online but not offline

Remote scripts, styles, fonts, media, connections, CSS imports, and module
imports are rejected by design. Install supported plugins explicitly so their
runtimes can be inlined. Generated images/videos must reference copied local
assets returned by Sfumato tools.

### Page browser inspection fails

Static validators check HTML, CSS, and JavaScript first. Browser inspection then
checks desktop 1440x900 and mobile 390x844 for uncaught errors, rejected
promises, missing images/videos, blank content, unrendered math, and horizontal
overflow.

Runtime errors or missing assets remaining after one repair fail the artifact
transaction. Responsive overflow may remain as a warning. Use the reported
viewport/kind/message in JSON `review.remaining_issues`.

## Hyperframe Video

### `renderer doctor hyperframe` reports unhealthy

Run:

```bash
sfumato renderer doctor hyperframe
```

Required for Sfumato's initial silent pipeline are the managed Hyperframes
executable, managed GSAP runtime, Node.js, FFmpeg, FFprobe, and a compatible
browser. Upstream doctor may also report optional transcription, local TTS,
MusicGen, or Docker features; these optional capabilities do not make the
Sfumato renderer unhealthy.

If managed files are missing, reinstall explicitly:

```bash
sfumato renderer install hyperframe
sfumato renderer doctor hyperframe
```

### Hyperframe lint reports missing composition metadata or timeline registry

Current Sfumato authoring requires:

- a root composition with `data-composition-id`, `data-width`, and
  `data-height`;
- local `./vendor/gsap.min.js`;
- one finite paused `gsap.timeline`;
- registration under `window.__timelines[compositionId]`;
- timed clips with stable IDs and timing/track metadata.

Sfumato validates this contract, runs `hyperframes lint` and `check`, and sends
the exact error plus source snapshot through one focused RFC 6902 repair. If the
second attempt fails, the transaction stops. Rebuild the binary after updating
source so the latest prompt and validator are used.

### Hyperframe tries to load GSAP from a CDN

Reinstall the renderer to get the managed pinned GSAP package. Generation
copies it into the staging source's `vendor/gsap.min.js`. Remote GSAP URLs are
forbidden by source validation.

### Hyperframe render fails after lint/check

Capture the complete stderr and verify system dependencies:

```bash
node --version
ffmpeg -version
ffprobe -version
sfumato renderer doctor hyperframe
```

The managed artifact is committed only after MP4 inspection succeeds, so a
render failure should not replace `current.json` with a broken revision.

## Manim Video

### Manim is not installed or unhealthy

```bash
uv --version
ffmpeg -version
ffprobe -version
sfumato renderer install manim
sfumato renderer doctor manim
```

The renderer has its own managed Python 3.12 environment; installing `manim`
globally does not satisfy that executable path.

### Manim code execution is denied

Generated Python requires explicit authorization:

```bash
sfumato generate video \
  --engine manim \
  --allow-code-execution \
  --duration 20 \
  --instruction "Animate the geometric meaning of convolution"
```

For a trusted project, persist `security.allow_manim = true`. This is consent to
execute validated generated code, not a strong sandbox. Sfumato rejects known
filesystem/network/process operations but cannot promise complete isolation.

### Generated Python is invalid

Sfumato requires `scene.py` with `class SfumatoScene`, runs Python compilation,
and rejects dangerous imports/operations before Manim. It can request one
focused source repair with the exact validation/render error.

## Remote Model Video

### Remote options are rejected

Remote video accepts provider-supported duration, resolution, aspect ratio,
audio policy, and seed. `--fps`, `--quality`, and
`--allow-code-execution` are local-engine options and fail before a paid call.

Check the profile and connector catalog:

```bash
sfumato model show remote-video
sfumato connector models openrouter
```

### Polling times out

OpenRouter video is asynchronous. Tune typed profile options rather than shell
loops:

```bash
sfumato model edit remote-video \
  --option video_poll_interval_seconds=5 \
  --option video_timeout_seconds=1200
```

Timeout/cancellation prevents endless polling. A provider may continue its
remote job after a local timeout; consult provider status/usage before blindly
submitting another paid request.

### Reference image is rejected

Local artifacts are sent as Base64 references only when the selected remote
model advertises support. Reduce references, select a compatible model, or use
Hyperframe/Manim. Standalone remote generation produces one clip; Sfumato does
not stitch multiple remote clips.

## Artifact Transactions And Publication

### Generation succeeded but publication failed

Managed commit and publication are separate. Inspect JSON `artifacts` and
`published_artifacts`: a valid revision may exist under `~/.sfumato/Projects`
even when copying to the vault failed.

Check output directory permissions, symlink boundaries, and free space. Rerun
with a valid `--out`; never move files into a guessed revision directory.

### Old published file/folder shape remains

Pages adapt between one HTML file and a directory when sidecar assets exist.
The atomic publisher removes the stale opposite shape after successful
publication. If a previous run was interrupted, inspect both
`<out>/_sfumato/pages/<slug>.html` and `<out>/_sfumato/pages/<slug>/` before
manual cleanup.

### A symlink path is rejected

Artifact and publication paths are canonicalized and checked against allowed
roots. Symlinks that escape a project, staging transaction, or publication root
are rejected intentionally. Use a real destination inside the allowed root.

### Find the current revision

Do not sort revision directories by name. Read the result JSON or the resource's
`current.json` pointer under:

```text
~/.sfumato/Projects/<project>/resources/<slides|pages|videos>/<resource-id>/
```

Each `revisions/<revision-id>/manifest.json` records parent revision, files,
models, prompts, tools/plugins, warnings, and hashes/provenance as applicable.

## TUI Problems

### Arrow keys do not move

Make sure focus is on the intended pane. In browse screens, `Tab` switches
between rows and actions; `Up`/`Down` moves rows and `Left`/`Right` moves
actions. Vim keys `h`, `j`, `k`, `l` are also supported in browse screens.

If a terminal sends unusual escape sequences, try the Vim bindings and confirm
the process is attached to a real interactive terminal.

### Cancel appears delayed

`Esc` requests cancellation. External model/render processes stop at operation
checkpoints and cancellation-aware waits, so UI completion can take a moment.
`Ctrl+C` requests cancellation and exits after the retained task is awaited.

### No TUI appears

When stdin/stdout is redirected or non-interactive, use explicit subcommands.
The no-command TUI is deliberately terminal-only.

## Reporting A Reproducible Failure

Include:

1. `sfumato --help` version/build context and operating system.
2. Exact command with secrets removed.
3. Selected project, connector kind, model profile names/capabilities, engine,
   theme, plugins, and tools.
4. `--dry-run` resolution result when it succeeds.
5. Full structured `--json` error or complete stderr.
6. Renderer doctor output for local video.
7. Prompt IDs/origins/hashes from the result manifest, not private prompt
   contents unless safe to share.
8. The managed revision manifest or staging-independent final artifact paths.

Never attach API keys, keyring values, authorization headers, or raw provider
responses that may contain credentials. Sfumato errors are sanitized, but logs
outside Sfumato may not be.
