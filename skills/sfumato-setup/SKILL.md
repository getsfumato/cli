---
name: sfumato-setup
description: Get sfumato from nothing to a first generation — user config, projects, connectors, model profiles, credentials and local renderers. Use when sfumato is not configured, when a provider or API key needs wiring, when a model profile is missing for a capability, when a renderer is missing, or when a run failed at the resolve stage.
allowed-tools: Bash(sfumato:*), Read
---

# Getting to a first generation

Four things must exist before a generation resolves: a user config, a project, a
connector, and a model profile bound to the `text` capability. Renderers are
needed only for the resource you actually generate.

```console
sfumato init user
sfumato init project university --path ~/Vault
sfumato connector setup ollama
sfumato model add local-text --connector ollama --id gemma3:latest \
  --capability text --capability code
sfumato model use text local-text --project university
sfumato generate slides --project university --instruction "..." --dry-run
```

The dry run at the end is the check that all four landed. A failure there is at
the `resolve` stage and costs nothing.

## Connector versus model profile

The distinction is the one most setup mistakes come from.

A **connector** is *how to reach a provider*: transport, endpoint,
authentication, and whatever native introspection that provider offers. A
**model profile** is *which model, for what*: one provider model ID, the
capabilities it may be selected for, and capability-specific options.

One connector backs many profiles. `connector models <name>` lists what the
provider offers — it discovers, it does not create profiles. Creating one is
`model add`.

## The six presets

| Preset | Transport | Authentication | Native operations |
|---|---|---|---|
| `ollama` | OpenAI-compatible local `/v1` | none | Local catalog, version, running processes. |
| `lmstudio` | OpenAI-compatible local `/v1` | none | Catalog via native `/api/v0/models`, which reports architecture, quantization, and load state that `/v1/models` omits. |
| `openrouter` | OpenAI-compatible text/image plus async video | keyring or `env:` reference | Catalog, key usage and limits. |
| `anthropic` | Native Messages API, `x-api-key` — not `chat/completions` | keyring or `env:` reference | Text only. |
| `codex` | Codex App Server, JSON-RPC over stdio | owned by `codex login` | Catalog, account, rate limits. |
| `elevenlabs` | Native speech, returns audio plus word-level timings | keyring or `env:` reference | Models and voices, tier, character budget. |

```console
sfumato connector setup openrouter
sfumato connector setup openrouter --name team-router --api-key-env TEAM_KEY
sfumato connector capabilities openrouter    # ask before assuming catalog support
```

`--name` is what lets two connectors share a preset — a personal and a team
OpenRouter account, say.

## Credentials

```console
sfumato connector login openrouter        # hidden prompt → OS keyring
sfumato connector auth-status openrouter  # can the reference be resolved?
sfumato connector logout openrouter       # deletes the secret, keeps the config
```

Global TOML stores only `stored:connector/<name>` — never the value.
`--api-key-env VAR` stores `env:VAR` instead, for CI where no keyring exists.

Two rules with no exceptions: **never put a raw key in `config.toml` or in shell
history**, and **never run `connector login codex`** — Codex owns its own
authentication, and Sfumato deliberately does not manage it.

## Capabilities are a contract

| Capability | Used for |
|---|---|
| `text` | Drafting, semantic review, planning, focused content repair. |
| `code` | Hyperframes HTML/JavaScript and Manim Python authoring. |
| `image` | `sfumato_image_gen` and artifact variant regeneration. |
| `video` | The remote video engine and the page video tool. |
| `speech` | Narration and the page audio tool. |
| `embedding` | Reserved. |

A profile selected for a stage that it does not declare fails at `resolve`,
before any provider call. That is the point: declaring a capability is a promise
the resolver checks.

## Adding profiles

```console
# text and code, one profile
sfumato model add codex --connector codex --id default \
  --capability text --capability code --option max_tool_rounds=8

# image
sfumato model add gpt-image --connector openrouter --id openai/gpt-image-2 \
  --capability image --option quality=high --option output_format=png

# remote video
sfumato model add remote-video --connector openrouter --id provider/video-model \
  --capability video --option video_timeout_seconds=900

# speech: --id is the synthesis model, speech_voice is who speaks
sfumato model add elevenlabs-speech --connector elevenlabs \
  --id eleven_multilingual_v2 --capability speech \
  --option speech_voice=<voice-id> --option speech_segment_gap_seconds=0.45
```

`sfumato connector models elevenlabs` lists both models and voices, and each row
says which of the two identifiers it fills in.

### Options that matter

Text and code: `temperature` (workflow default `0.4`), `max_tokens` (default
`4000`), `max_tool_rounds` (default `8`), `top_p`, `seed`.

Anthropic is the exception on both: it rejects sampling parameters, so `init`
leaves `temperature` unset, and thinking shares the output budget, so
`max_tokens` is left unset and the adapter defaults to `16000`.

Image: `quality`, `background`, `size`, `aspect_ratio`, `output_format`.
Video: `video_duration_seconds`, `video_resolution`, `video_aspect_ratio`,
`video_audio`, `video_seed`, `video_poll_interval_seconds`,
`video_timeout_seconds`.

Unknown keys and wrong types are rejected before anything is written.

### Editing and removing

```console
sfumato model edit codex --option max_tokens=8000
sfumato model remove old-profile
```

`edit` merges options by key but **any `--capability` replaces the whole set** —
passing one capability to a two-capability profile drops the other. An edit that
removes a capability something depends on is refused, as is removing a profile a
default or role still points at. Reassign first.

## Selecting defaults

```console
sfumato model use text codex                        # user layer
sfumato model use text codex --project university   # project layer
sfumato model use reviewer grok-latest --project university
```

Omitting `--project` writes the **user** default. The selector is a capability or
the `reviewer` role.

## Renderers

Install only what the resource needs. Sfumato never falls back between them, and
`install` is the only command allowed to run npm/uv and download.

| Renderer | Needed for | Also requires |
|---|---|---|
| `pagedjs` | `generate document` PDFs | Node, Chrome |
| `hyperframe` | `generate video --engine hyperframe` | Node, FFmpeg, FFprobe, Chrome |
| `manim` | `generate video --engine manim` | `uv`, FFmpeg, FFprobe |

```console
sfumato renderer install hyperframe
sfumato renderer doctor            # all of them
```

Not managed by Sfumato, and needed separately: **Marp CLI** and a Chromium-family
browser for slide PDFs, and **`mmdc`** for Mermaid in slides and documents.

## Executing generated Python

`manim` and the `chart-gen` tool both run Python the model wrote. Both need
explicit authorization:

```console
sfumato config set security.allow_python true --scope project --project university
```

Or `--allow-code-execution` for one run. Extra packages beyond matplotlib, numpy,
and sympy must be listed in `security.python_packages`. Sfumato rejects dangerous
imports and compiles each module before running it in a managed environment —
that is a validation gate, **not a strong sandbox**. Decide accordingly.

## Verifying before you spend anything

```console
sfumato project show university
sfumato config show --scope effective --project university
sfumato connector auth-status openrouter
sfumato model list
sfumato tool list --project university
sfumato renderer doctor
sfumato prompt validate --project university
sfumato generate slides --project university --instruction "..." --dry-run
```

## When setup is wrong

| Symptom | Cause |
|---|---|
| `config` at `resolve`, naming a capability | No profile is selected for it. `model use <capability> <profile>`. |
| `config` at `resolve`, naming a profile | The profile does not declare the capability the stage needs. `model show` it. |
| `provider` + `unavailable` | The connector cannot be reached. `connector status`, then `auth-status`. |
| `not_found` on a theme or template | `theme list` / `template list` — the name is wrong. |
| `render` + `unavailable` | `renderer doctor` names the missing piece. |
| Codex fails to authenticate | `codex login`. Sfumato does not manage it. |
