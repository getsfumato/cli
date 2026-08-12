# Configuration Schema v5

**Implementation status:** active in v0.2.

Schema v5 separates page composition defaults, generative tools, and generated
code authorization. Unknown fields and versions other than 5 are rejected,
except schema v4 which is migrated once with an adjacent `.v4.bak` backup and
an atomic replacement.

## Documents

| Scope | Default path | Purpose |
| --- | --- | --- |
| Global | `<platform-config>/sfumato/config.toml` | User profile, connectors, models, defaults, roles, and Marp. |
| Registry | `<platform-config>/sfumato/projects.toml` | Registered project roots and active project. |
| Project | `<project-root>/.sfumato/project.toml` | Portable theme, publication, model roles, page defaults, tools, and security. |

Every document starts with `schema_version = 5`. Configuration contains only
indirect `stored:` or `env:` secret references; credentials are managed through
`sfumato connector login`, `auth-status`, and `logout`.

## Project Example

```toml
schema_version = 5
name = "university"
theme = "gruvbox"
publish_dir = "Published"

[model_defaults]
text = "codex"
code = "codex"
image = "gpt-image"
video = "openrouter-video"
speech = "elevenlabs-speech"

[model_roles]
reviewer = "grok-latest"

[page]
ui = "shadcn"
plugins = ["motion"]

[generation_tools]
image_gen = true
video_gen = false
audio_gen = true
chart_gen = true

[security]
allow_python = false
python_packages = []

[knowledge]
backend = "filesystem"

[marp]
pdf = true

[browser]
path = "/usr/bin/chromium"
```

`browser.path` names the Chromium-family executable to use. It is only needed when
discovery cannot find one — the browser is looked up in `SFUMATO_BROWSER`,
`PUPPETEER_EXECUTABLE_PATH` and `CHROME_PATH`, then on `PATH`, then in the
platform's well-known locations. A configured path that does not exist is an error
rather than a silent fall back to discovery, because rendering with a browser the
user did not choose is worse than saying the chosen one is gone.

> It used to be `marp.browser_path`, which named one caller of four: pages,
> documents and Mermaid diagrams launch the same browser. **The old key is still
> read**, so nothing has to change and the schema version is unaffected. Where both
> appear, `browser.path` wins. A project's `[marp]` still replaces the global one
> wholesale, as it always did.

`page.ui` is exclusive. Enabling another UI library replaces it. Entries in
`page.plugins` must be utility plugins; runtime dependencies are selected
transitively and cannot be enabled directly.

Generation tools are independent from page plugins. `image_gen` may be exposed
to slides, pages, and video planning; `video_gen` may be exposed only to page
drafting and uses the configured remote video profile. `audio_gen` exposes a
speech tool to page drafting and turns on narration for Hyperframe videos, where
Sfumato speaks the plan's per-scene lines itself instead of offering a tool.
Command `--tool` and `--disable-tool` overrides win over project defaults. With
no explicit value, a tool is enabled when its capability has a configured model
default, except `video_gen`, which stays opt-in because it spends a remote render.

`security.allow_python` is a persistent opt-in to executing generated Python. It
covers every workflow that runs code Sfumato wrote: the Manim video engine and
the `chart_gen` tool. Projects written before the setting was renamed still say
`allow_manim`, which is read as the same consent. The one-command alternative is
`generate video --engine manim --allow-code-execution`. It is an authorization
boundary, not a strong sandbox.

`security.python_packages` lists requirements a project permits on top of the
pinned base environments, matched by package name so a pin does not have to be
repeated. It is empty by default: layering a package installs it from an index
during generation, which is the project's decision rather than the model's.

## Knowledge

`[knowledge]` decides where a project's resources may draw their claims from. It
is project-scoped and has no global counterpart: a brain belongs to the work, not
to the machine, and two projects on one machine routinely ground differently. A
project file without the table reads as `filesystem`, which is what every project
did before the table existed.

```toml
[knowledge]
backend = "vitruvio"                     # "filesystem" (default) | "vitruvio"
project = "facultad"                     # registered Vitruvio project holding the brain
brain = "algebra"                        # brain name from vitruvio.toml, or a path
config = "../vitruvio/vitruvio.toml"     # instead of `project`: one vitruvio.toml, verbatim
executable = "~/.local/bin/vitruvio"     # optional; "vitruvio" on PATH otherwise
actor = "sfumato"                        # recorded against each query
memory_types = ["canonical", "semantic"] # default filter, optional
include_superseded = false
default_limit = 10
max_limit = 50
timeout_seconds = 60
```

A Vitruvio brain is addressed in two steps — which project, then which brain
within it — because a brain name means nothing until it is known whose
vocabulary it belongs to. `project` names one Vitruvio has registered, which is
what makes the address work from any directory; `config` is the alternative for
a project that was never registered, and points at its `vitruvio.toml`. Set one
or the other: Vitruvio takes `config` verbatim and never consults its registry
afterwards, so naming both is refused rather than left to mean nothing.

Naming neither is allowed, and means Vitruvio walks up from wherever Sfumato was
run to find a `vitruvio.toml`. That is fine for a brain inside the Sfumato
project's own tree and misleading everywhere else, because it makes the working
directory decide which brain a name refers to.

`--brain-project` and `--brain` override the two keys for one run, on every
`generate` command:

```console
sfumato generate slides --project university --brain simulacion --instruction "..."
sfumato generate slides --project university \
    --brain-project ethicompass --brain metrica-a --instruction "..."
```

`--brain-project` also clears `config` for that run: the two are alternatives, and
Vitruvio honours a config file over a project name, so keeping the file would
leave the flag doing nothing. Neither flag will *ground* a run — on a project
still set to `filesystem` they are refused rather than switching the backend,
because that changes where every claim in the resource may come from and refuses
the source paths the command was called with. `sfumato edit slides` has neither
flag: it works from the deck it is handed and reaches no knowledge source.

With `backend = "vitruvio"`, `brain` is required and generation changes in
exactly two places. The model is offered `sfumato_search_brain` instead of
`sfumato_list_directory` and `sfumato_read_file`, and the prompt carries an
inventory of the brain — its modules, their block counts, the filters available —
instead of an index of files. Everything after that is identical: drafting,
validation, diagrams, layout, review, rendering, and publishing.

Source paths are **refused** under a brain rather than ignored. A silently
dropped path would leave you believing a file grounded the resource when nothing
did, and the resource looks the same either way.

`default_limit` is used when the model asks for no particular number of matches;
`max_limit` caps what it may ask for, and a clamp is reported back to the model
rather than applied silently. `timeout_seconds` bounds one brain invocation, so a
brain that hangs cannot hang the run.

## Model Options

Profiles retain typed text and image options and may add video or speech options:

```toml
[models.openrouter-video]
connector = "openrouter"
model = "provider/video-model"
capabilities = ["video"]

[models.openrouter-video.options]
video_duration_seconds = 5
video_resolution = "720p"
video_aspect_ratio = "16:9"
video_audio = "auto"
video_seed = 42
video_poll_interval_seconds = 5
video_timeout_seconds = 900
```

```toml
[models.elevenlabs-speech]
connector = "elevenlabs"
model = "eleven_multilingual_v2"
capabilities = ["speech"]

[models.elevenlabs-speech.options]
speech_voice = "21m00Tcm4TlvDq8ikWAM"
speech_output_format = "mp3_44100_128"
speech_language = "es"
speech_stability = 0.5
speech_similarity_boost = 0.75
speech_style = 0.0
speech_speed = 1.0
speech_speaker_boost = true
speech_segment_gap_seconds = 0.45
```

`speech_voice` is the other half of a speech profile's identity: `model` selects
the synthesis model and `speech_voice` selects who speaks.
`speech_segment_gap_seconds` is the silence held after each spoken passage, which
also decides how much room a narrated scene gets beyond its own words.

Model resolution remains command override, project default, then global
default. Reviewer resolution remains command role override, project role,
global role, then the draft text profile.

## Migration

Schema v4 project `plugins` are classified during migration. `shadcn` and
`materialui` become the exclusive `page.ui` selection, with the last legacy UI
entry winning; `react` and `react-dom` remain internal dependencies; other
entries become `page.plugins`. Empty `generation_tools` and
`security.allow_python = false` are added. Global and registry documents retain
their existing values and receive schema version 5.

Versions 1 through 3, a missing version, and future versions fail without
rewriting. Reads of an already-v5 document are side-effect free. Writes are
validated, locked, revision-aware, synced, and atomic.

## Resolution

Values resolve from lowest to highest precedence:

1. global configuration;
2. selected project configuration;
3. command overrides.

Publication paths may be absolute or project-relative. Managed immutable
artifacts always remain under `~/.sfumato/Projects`; publication copies only
processed PDF, HTML, or MP4 output into the requested project-visible folder.
