# Connectors And Models

## Separation Of Responsibilities

A connector describes transport, endpoint, authentication, and provider-native
introspection. A model profile selects one provider model ID, declares what it
can do, and stores capability-specific inference options.

One connector can back many profiles. Generation resolves profiles, then asks
their connector to construct the matching provider implementation.

## Connector Presets

| Preset | Generation transport | Authentication | Native operations |
| --- | --- | --- | --- |
| `ollama` | OpenAI-compatible local `/v1` | None by default | Local model catalog, version, and running processes. |
| `openrouter` | OpenAI-compatible text/image plus asynchronous video API | OS keyring by default or explicit environment reference | Model catalog and key usage/limits. |
| `codex` | Codex App Server JSON-RPC over stdio | Owned by `codex login` | Model catalog, account, and rate limits. |
| `elevenlabs` | Native speech synthesis returning audio plus word-level timings | OS keyring by default or explicit environment reference | Model and voice catalog, subscription tier, and character budget. |

## Connector Commands

### Setup

```bash
sfumato connector setup <ollama|lmstudio|openrouter|anthropic|codex|elevenlabs> \
  [--name <connector-name>] \
  [--api-key-env <environment-variable>]
```

- `--name` permits multiple named connectors using the same preset.
- Without `--name`, the connector name equals the preset.
- `--api-key-env` stores an indirect `env:VARIABLE` reference instead of using
  the native credential store.
- `codex` ignores Sfumato credential management and delegates authentication to
  the Codex application.

Examples:

```bash
sfumato connector setup ollama
sfumato connector setup openrouter
sfumato connector setup openrouter --name team-router --api-key-env TEAM_OPENROUTER_KEY
sfumato connector setup codex
sfumato connector setup elevenlabs
```

`elevenlabs` only speaks. It is deliberately absent from first-run setup, which
asks for the connector that will draft; add it afterwards alongside a text
connector.

### List And Show

```bash
sfumato connector list
sfumato connector show <name>
```

`list` prints configured connectors in a table. `show` prints one redacted
configuration. Neither command reveals credential values.

### Capabilities

```bash
sfumato connector capabilities <name>
```

Reports generation and native-management features exposed by that connector.
Agents should call this before assuming catalog or status support.

### Models

```bash
sfumato connector models <name>
```

Queries the connector-native model catalog:

- Ollama uses its local tags endpoint.
- OpenRouter returns model IDs, modalities, context, pricing, and supported parameters.
- Codex App Server uses `model/list` and identifies the default model.
- Generic OpenAI-compatible connectors may not expose a catalog.

This command discovers provider models; it does not create Sfumato profiles.

### Status

```bash
sfumato connector status <name>
```

Queries native runtime or account state. Ollama reports local runtime details,
OpenRouter reports key usage and limits, and Codex reports account/rate limits.

### Login, Authentication Status, And Logout

```bash
sfumato connector login <name>
sfumato connector auth-status <name>
sfumato connector logout <name>
```

- `login` reads the credential with a hidden prompt and writes it to the native
  operating-system credential store.
- Global TOML stores only `stored:connector/<name>`.
- `auth-status` reports whether the referenced credential can be resolved.
- `logout` deletes the stored secret but preserves connector configuration.
- These operations are not supported for the Codex connector; use `codex login`.

Never place raw API keys in shell history or `config.toml`.

## Model Capabilities

Profiles accept one or more of:

| Capability | Current use |
| --- | --- |
| `text` | Drafting, semantic review, planning, and focused content repair. |
| `code` | Hyperframes HTML/JavaScript and Manim Python authoring. |
| `image` | `sfumato_image_gen` and project artifact variant regeneration. |
| `video` | Remote standalone video and page `sfumato_video_gen`. |
| `speech` | Hyperframe narration and the page `sfumato_audio_gen` tool. |
| `embedding` | Reserved for future retrieval workflows. |

Declaring a capability is a contract. A profile selected for a stage must have
the required capability or resolution fails before provider invocation.

## Model Commands

### Add

```bash
sfumato model add <profile-name> \
  --connector <connector> \
  --id <provider-model-id> \
  --capability <capability>... \
  [--option <key=value>]...
```

Example text/code profile:

```bash
sfumato model add codex \
  --connector codex \
  --id default \
  --capability text \
  --capability code \
  --option max_tool_rounds=8
```

Example image profile:

```bash
sfumato model add gpt-image \
  --connector openrouter \
  --id openai/gpt-image-2 \
  --capability image \
  --option quality=high \
  --option output_format=png
```

Example remote video profile:

```bash
sfumato model add remote-video \
  --connector openrouter \
  --id provider/video-model \
  --capability video \
  --option video_duration_seconds=5 \
  --option video_resolution=720p \
  --option video_aspect_ratio=16:9 \
  --option video_audio=auto \
  --option video_poll_interval_seconds=5 \
  --option video_timeout_seconds=900
```

Example speech profile. A speech profile carries two identifiers: `--id` selects
the synthesis model and `speech_voice` selects who speaks. Run `sfumato connector
models elevenlabs` to see both — each row says which one it fills in.

```bash
sfumato model add elevenlabs-speech \
  --connector elevenlabs \
  --id eleven_multilingual_v2 \
  --capability speech \
  --option speech_voice=21m00Tcm4TlvDq8ikWAM \
  --option speech_output_format=mp3_44100_128 \
  --option speech_stability=0.5 \
  --option speech_similarity_boost=0.75 \
  --option speech_segment_gap_seconds=0.45
```

### Edit

```bash
sfumato model edit <profile-name> \
  [--connector <connector>] \
  [--id <provider-model-id>] \
  [--capability <capability>]... \
  [--option <key=value>]...
```

- Only supplied scalar fields change.
- Supplying one or more `--capability` flags replaces the complete capability set.
- Options merge by key with existing options.
- An edit is rejected if it removes a capability required by a user/project default or reviewer role.

### List, Show, And Remove

```bash
sfumato model list
sfumato model show <profile-name>
sfumato model remove <profile-name>
```

Removal is rejected while any global or project default/role references the
profile. Reassign those references first.

### Select Defaults And Roles

```bash
sfumato model use <selector> <profile> [--project <name>]
```

Selectors are capabilities (`text`, `code`, `image`, `video`, `speech`,
`embedding`) or the `reviewer` role.

Without `--project`, the command updates the global user default. With
`--project`, it updates that project's portable configuration.

```bash
sfumato model use text codex
sfumato model use text codex --project university
sfumato model use code codex --project university
sfumato model use image gpt-image --project university
sfumato model use video remote-video --project university
sfumato model use reviewer grok-latest --project university
```

## Supported Model Options

All options use repeatable `--option key=value` arguments.

### Text And Code

| Key | Type | Default/use |
| --- | --- | --- |
| `temperature` | float | Sampling temperature; workflow default is `0.4`. Inert on `anthropic`, which rejects sampling parameters, so `sfumato init` leaves it unset there. |
| `max_tokens` | positive integer | Provider output budget; workflow default is `4000`. Left unset for `anthropic`, where thinking shares this budget and the adapter defaults to `16000` (capped at `32000` without streaming). |
| `max_tool_rounds` | positive integer | Maximum model/tool cycles; default is `8`. |
| `top_p` | float | Optional nucleus sampling threshold. |
| `seed` | integer | Optional deterministic seed where supported. |

### Image

| Key | Type | Meaning |
| --- | --- | --- |
| `quality` | string | Provider quality, for example `high`. |
| `background` | string | Provider background policy, for example `transparent`. |
| `size` | string | Provider image dimensions. |
| `aspect_ratio` | string | Provider image aspect ratio. |
| `output_format` | string | Requested format such as `png` or `webp`. |

### Video

| Key | Type | Meaning |
| --- | --- | --- |
| `video_duration_seconds` | positive integer | Default duration for the page video tool. |
| `video_resolution` | string | Remote provider resolution such as `720p`. |
| `video_aspect_ratio` | string | Remote provider ratio such as `16:9`. |
| `video_audio` | `auto`, `on`, or `off` | Native audio policy. |
| `video_seed` | integer | Provider seed where supported. |
| `video_poll_interval_seconds` | positive integer | Delay between async status checks. |
| `video_timeout_seconds` | positive integer | Maximum async generation wait. |

Unsupported option keys or invalid value types are rejected before writing.

## Generation-Time Overrides

Generation commands accept repeatable named overrides:

```bash
--model text=codex --model code=codex --model image=gpt-image
```

The left side is a capability, not a connector. The right side is a registered
profile name, not a provider model ID.

Use `--review-model <profile>` for the reviewer role. This override does not
change persisted project settings.
