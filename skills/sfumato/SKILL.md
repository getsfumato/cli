---
name: sfumato
description: Generate slides, documents, pages and videos from notes through the sfumato CLI. Use when the user asks to make a deck, a handout, an interactive page or a short video from their material, when a generation failed, when configuring projects, connectors, models or themes, or when they mention sfumato, Marp, Hyperframes, Manim, or an Obsidian vault of course notes.
allowed-tools: Bash(sfumato:*), Read, Glob, Grep
---

# Sfumato

Sfumato turns material you already have — notes, a vault, a repository, or a
knowledge brain — into a finished **resource**: a Marp deck, a paginated
document, an offline HTML page, or an MP4. A model drafts the content;
Sfumato owns everything around it — the prompt, the validation, the renderers,
the artifact store, and what is allowed to reach the model at all.

The one thing to internalise first: **a generation is a pipeline with gates, not
a model call.** Between the draft and the file on disk sit structural
validation, semantic review, real browser measurement, focused repair, and an
immutable commit. Most of what goes wrong goes wrong at a gate, and the gate
names itself in `error.stage`. Reading that field is the difference between
fixing the cause and re-rolling the dice.

```console
sfumato init user                                     # once per machine
sfumato init project university --path ~/Vault        # once per body of work
sfumato generate slides ./notes --instruction "..."   # every time after that
```

## Four resources, one shape

| Resource | Output | Reach for it when |
|---|---|---|
| `slides` | Marp Markdown + PDF | The material is being *presented* — fixed boxes, one idea per slide. |
| `document` | Markdown + paginated PDF | The material is being *read or printed* — prose flowing across numbered pages. |
| `page` | An HTML page that fetches nothing | The material is *interactive* — it needs JavaScript, a simulation, a control. |
| `video` | MP4 | The material is *motion* — something that only makes sense while it changes. |

They differ in their renderer and their failure modes, not in their shape. All
four resolve the same configuration, read the same sources, draft, review,
repair, and commit one immutable revision. Learn one and you have learned the
other three — see [`sfumato-generate`](../sfumato-generate/SKILL.md).

## The five things a generation resolves

Every run answers these before it spends a token, and every one of them can be
overridden for a single run without editing a file:

1. **Project** — which body of work, and therefore which sources, artifacts, and
   settings. `--project`.
2. **Theme** — colours, fonts, and one renderer adapter per resource kind.
   Exactly one per project. `--theme`.
3. **Models** — a profile per capability (`text`, `code`, `image`, `video`,
   `speech`) plus the `reviewer` role. `--model text=...`, `--review-model`.
4. **Grounding** — the filesystem or a Vitruvio brain. Decided by the project;
   `--brain` and `--brain-project` point one run elsewhere.
5. **Tools** — which model-facing functions are offered. `--tool`,
   `--disable-tool`.

`--dry-run` resolves all five and renders the prompt without calling a provider.
It is the cheapest way to find out what a command is actually about to do.

## Configuration is three layers

User (global) → project (portable, in the vault) → this run (flags). Later wins.

```console
sfumato config show --scope effective --project university   # what a run will see
sfumato config show --scope project --project university     # what the vault carries
sfumato config set security.allow_python true --scope project --project university
```

The project file lives at `<project-root>/.sfumato/project.toml` and travels
with the directory, which is why a project moved between machines keeps its
theme and its models. The user layer lives under platform configuration and
holds connectors, model profiles, and credentials-by-reference.

## Where output goes

Two destinations, and they are not alternatives:

- **The managed store** is authoritative. Every successful run commits one
  immutable revision under `~/.sfumato/Projects/<project>/resources/<kind>/`,
  with a manifest recording models, prompts, hashes, and every committed file.
  Nothing overwrites a previous revision.
- **Publication** is a copy for humans, under `<folder>/_sfumato/<kind>/<slug>/`,
  requested with `--out` or a project `publish_dir`. It happens *after* the
  managed commit, so a publication failure still leaves a valid revision.

Never construct a revision path by hand. Read `artifacts`,
`published_artifacts`, `markdown_path`, `pdf_path`, `html_path`, or `video_path`
out of the `--json` result.

## Always `--json` when something other than a person is reading

Success is one pretty-printed JSON object on stdout. A typed failure is also one
JSON object on stdout, and the exit status is non-zero. Progress events are
suppressed. Branch on the exit status first, then on `error.class`:

```json
{ "error": { "code": "provider", "class": "retry", "retryable": true,
             "stage": "draft", "message": "...", "details": {} } }
```

The full taxonomy — nine codes, six classes, eleven stages, and what each one
means you should *do* — is in [error-contract.md](references/error-contract.md).
Read it before writing any retry logic, because "retryable" does not mean
"retry the same thing".

## Cost discipline

A generation calls a paid model several times: a draft, a review, one repair per
defect, possibly a scene per beat for a video. Three habits, in order of how
much they save:

1. `--dry-run` first, always. It costs nothing and catches the wrong project,
   the missing theme, the unconfigured profile.
2. `--no-review` while iterating on wording. It skips semantic review and
   focused repair — deterministic validation still runs, so you cannot commit
   something broken, only something unpolished.
3. Small `--duration` and `--quality draft` while iterating on a video. Hyperframe
   authors one model call per scene; a long plan with many short beats is the
   expensive shape.

## Reading a result honestly

- **`warnings` is not decoration.** A deck can commit without its PDF when Marp
  is missing, a page can commit with responsive overflow, a video can commit with
  frame defects that survived repair. All three are successes with a warning, and
  all three look identical to a clean run if you only check the exit status.
- **`--no-review` narrows what was checked, not what was claimed.** Say so when
  reporting the result.
- **A dry run proves resolution, not success.** It does not prove a provider will
  answer, a renderer will run, or a browser exists.

## Where to go next

| Skill | Covers |
|---|---|
| [`sfumato-cli`](../sfumato-cli/SKILL.md) | Every command group, every command, the flag that decides the outcome. |
| [`sfumato-generate`](../sfumato-generate/SKILL.md) | The four resources in depth: pipelines, flags, failure modes. |
| [`sfumato-setup`](../sfumato-setup/SKILL.md) | Zero to a first generation: init, connectors, models, credentials, renderers. |
| [`sfumato-library`](../sfumato-library/SKILL.md) | Themes, templates, project artifacts, prompts, plugins, tools. |
| [`sfumato-knowledge`](../sfumato-knowledge/SKILL.md) | Filesystem grounding versus a Vitruvio brain. |
| [`sfumato-automation`](../sfumato-automation/SKILL.md) | Driving Sfumato from a script or an agent. |

References carried by this skill:
[cli-reference.md](references/cli-reference.md) (every command, generated from
the parser), [error-contract.md](references/error-contract.md),
[artifact-store.md](references/artifact-store.md).
