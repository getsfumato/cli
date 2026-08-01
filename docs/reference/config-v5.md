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

[security]
allow_python = false
python_packages = []

[marp]
pdf = true
```

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
