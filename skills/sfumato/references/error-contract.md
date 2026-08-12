# The error contract

What `--json` puts on stdout when something fails, and what each field means you
should *do*. Every generation command and `edit slides` follow this.

```json
{
  "error": {
    "code": "provider",
    "class": "retry",
    "retryable": true,
    "stage": "draft",
    "message": "sanitized provider message",
    "details": {}
  }
}
```

The exit status is non-zero whenever this object is printed. Check the status
first; a caller that parses stdout without checking the status will read a
success object as though nothing were wrong.

## Two shapes, and you must accept both

The object above comes from the typed core. Validation that happens in the
presentation layer — before a use case is entered at all — returns a smaller
one:

```json
{ "error": { "message": "Instruction cannot be empty" } }
```

Only `message` is guaranteed. Treat a missing `retryable` as `false`: it means
the failure was rejected before anything could classify it, which is always a
"fix the input" case.

## `code` — what kind of thing failed

| Code | Meaning |
|---|---|
| `config` | Configuration could not be loaded, resolved, or validated. |
| `validation` | Input or generated data violated a domain invariant. |
| `not_found` | A named project, model, theme, prompt, template, or artifact does not exist. |
| `provider` | A model provider request failed. |
| `tool` | A model-requested tool failed. |
| `render` | A renderer or layout inspector failed. |
| `artifact` | Staging, commit, or publication failed. |
| `cancelled` | The caller or a deadline stopped the operation. |
| `internal` | An unexpected application failure. Report it. |

## `class` — whether to try again, and how

This is the field a retry loop branches on. `retryable` is a convenience derived
from it, and it is deliberately not a licence to repeat the same call.

| Class | Retryable | What "retry" actually means |
|---|---|---|
| `retry` | yes | The same request, after bounded backoff. A transient provider or network fault. |
| `context_limit` | yes | **Compact first.** Fewer sources, a narrower instruction, a shorter duration. Sfumato already made its own one-shot compacted attempt before surfacing this. |
| `invalid_output` | yes | The model produced something that did not validate. Retry with corrective feedback, or a stronger profile — never an identical blind re-roll. |
| `unavailable` | yes | A dependency is missing or down. Fix the dependency: `renderer doctor`, `connector status`, install the missing CLI. |
| `cancelled` | no | Deliberate. Nothing was committed. |
| `permanent` | no | Unchanged input cannot succeed. Change the configuration or the request. |

## `stage` — where in the pipeline it happened

The most useful field for a human, because it says which part of the run to look
at. Eleven, in pipeline order:

| Stage | What was happening |
|---|---|
| `resolve` | Resolving configuration, project, models, themes. Nothing was spent. |
| `read_sources` | Discovering and reading source material. |
| `render_prompt` | Resolving and rendering prompt templates. A bad prompt override lands here. |
| `draft` | Generating the initial draft. |
| `edit` | Applying a focused edit to an existing resource. |
| `review` | Semantic review. |
| `inspect_layout` | Measuring rendered layout in a real browser. |
| `repair` | Repairing invalid content, diagrams, or layout. |
| `render` | Rendering final files — Marp, Paged.js, Hyperframes, Manim, ffmpeg. |
| `commit_artifacts` | Validating and committing the revision. |
| `publish` | Copying to the publication directory. |

Two stages are worth knowing for what they imply about cost. A failure at
`resolve` or `read_sources` spent nothing. A failure at `publish` means the
managed revision **is committed and valid** — only the copy failed, and you can
publish it later rather than regenerating.

## Reading the pair

The code says what, the class says whether to try again. The combination is what
carries the action:

- `provider` + `retry` — backoff and repeat.
- `provider` + `context_limit` — the material is too large for this profile.
  Cut sources or raise `max_tokens` on the profile.
- `validation` + `invalid_output` — the model wrote something malformed. Sfumato
  already retried once with the exact complaint; a second identical attempt is
  waste. Change the profile or simplify the instruction.
- `render` + `unavailable` — a renderer or browser is missing. `sfumato renderer
  doctor` names it.
- `config` + `permanent` — the message names the key. Fix it and re-run.
- `not_found` + `permanent` — a name is wrong. The `list` command for that
  subsystem shows the real ones.

## What is not an error

A run that commits with warnings exits zero. Missing Marp can still commit the
Markdown, a page with responsive overflow can still commit, and a video with
frame defects that survived repair still ships. Those are in `warnings` and in
result fields such as `review.frame_defects`, not in `error`. A caller that only
checks the exit status will report all three as clean.
