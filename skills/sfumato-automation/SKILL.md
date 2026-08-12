---
name: sfumato-automation
description: Drive sfumato from a script or an agent — JSON output, dry runs, error handling, retries, cancellation and atomicity. Use when automating sfumato, when parsing its output, when deciding whether a failure is worth retrying, or when a generation must be reproducible from a non-interactive process.
allowed-tools: Bash(sfumato:*), Read
---

# Driving Sfumato non-interactively

Running `sfumato` with no subcommand opens the terminal interface, and only when
stdin and stdout are both a terminal. Everything below assumes you are passing a
subcommand.

## The five rules

1. **Always `--project <NAME>`.** The active project is a convenience for a
   human at a keyboard. Another terminal can change it while your script runs.
2. **Always `--json`** when something other than a person reads the output.
3. **Check the exit status before parsing.** Non-zero means the JSON object on
   stdout is an error, not a result.
4. **Take paths from the result**, never by constructing a revision directory.
5. **Never infer success from a path in the logs.** Require status zero *and* a
   parsed object.

## `--json`

Success is one pretty-printed JSON object on stdout, with progress events
suppressed. A typed failure is also one object on stdout, and the status is
non-zero.

```bash
sfumato generate page \
  --project university \
  --instruction "Explain convolution interactively" \
  --json > result.json
```

Available on all four `generate` commands, on `edit slides`, and on `video
preview` / `video approve`.

## Handling a failure

```json
{ "error": { "code": "provider", "class": "retry", "retryable": true,
             "stage": "draft", "message": "...", "details": {} } }
```

Presentation-layer validation returns a smaller shape with only `message`. Accept
both, and treat a missing `retryable` as `false`.

The decision procedure:

1. Exit status zero? Parse the result and read `warnings`.
2. Otherwise parse stdout. If `retryable` is absent or false, **change something**
   before retrying.
3. If true, branch on `class`:
   - `retry` — the same request after bounded backoff.
   - `context_limit` — compact first. Fewer sources, narrower instruction,
     shorter duration. Sfumato already made its own compacted attempt.
   - `invalid_output` — the model produced something malformed. Sfumato already
     re-asked once with the exact complaint; a second identical attempt is waste.
     Change the profile or simplify the request.
   - `unavailable` — fix the dependency. `renderer doctor`, `connector status`.
4. Use `stage` to know what was reached: `resolve` and `read_sources` spent
   nothing; `publish` means the managed revision **is committed and valid** and
   only the copy failed.

Full taxonomy: [error-contract.md](../sfumato/references/error-contract.md).

## Success is not always clean

A run that commits with warnings exits zero. A caller that checks only the status
will report all of these as clean:

- A deck committed without its PDF because Marp is missing.
- A page committed with responsive overflow.
- A video whose duration grew because narration needed more room.
- A video with frame defects that survived repair.

Read `warnings` on every result, and the resource-specific fields —
`review.frame_defects`, `review.context_compaction`, `layout_issues`.

## Reading paths

| Field | Where |
|---|---|
| `artifacts` | Every committed file in the managed revision. |
| `published_artifacts` | Everything written to the publication directory. |
| `markdown_path`, `pdf_path` | Slides, `edit slides`. |
| `html_path` | Pages. |
| `video_path` | Videos. |

A successful `edit slides` commits a **new** revision. The path you passed in is
not the path that came out.

## Dry runs

`--dry-run` resolves the project, theme, models, tools, plugins or renderer
requirements, the source bundle, `SFUMATO.md`, artifacts, and prompt templates,
and prints the rendered prompt. No provider, browser, renderer, or artifact-store
work happens.

It is the right preflight and the wrong oracle: it proves resolution, not that a
provider will answer or a renderer will run. Do not parse its human text in
production — use it for diagnostics, then run the real command with `--json`.

## A deterministic sequence

```bash
sfumato project show university
sfumato config show --scope effective --project university
sfumato connector auth-status openrouter
sfumato model show codex
sfumato prompt validate --project university
sfumato generate slides --project university --instruction "..." --dry-run
sfumato generate slides --project university --instruction "..." --json
```

Add `plugin list --project`, `tool list --project`, or `renderer doctor` when the
resource needs those subsystems.

## Timeouts

`--timeout <SECONDS>` is global and bounds anything that waits outside the
process — a provider request, a renderer, generated Python. Unbounded when
omitted, which is rarely what an automated caller wants. A remote video job can
legitimately run for many minutes; size the timeout to the resource rather than
to a habit.

## Quoting

Quote paths and instructions containing spaces, apostrophes, shell symbols, or
non-ASCII text:

```bash
sfumato generate slides \
  "/Users/alex/Documents/Alex's Notebook/Facultad/notes" \
  --instruction "Explicá la transformada de Laplace paso a paso" \
  --project university \
  --json
```

Never interpolate untrusted text into a command string. A process-spawning agent
should pass each argument separately and skip the shell entirely.

## Cancellation and atomicity

Workflows check cancellation and deadlines at stage boundaries and around
external operations. Managed resources are staged and committed only after
validation, so **a failed or cancelled transaction never becomes the current
revision**.

Publication happens after the managed commit. A publication failure therefore
leaves a valid managed revision and returns a warning or a publish-stage error —
regenerating in that situation pays for the whole run again to fix a copy.

## Cost

Each generation makes several paid calls: a draft, a review, one repair per
defect, and for video one authoring call per scene. In rough order of savings:

1. `--dry-run` first. Free, and catches the wrong project or a missing profile.
2. `--no-review` while iterating on wording. Deterministic validation still runs.
3. For video, small `--duration` and `--quality draft`. Cost scales with **scene
   count**, so a plan of many short beats is the expensive shape.
4. Prefer `chart-gen` over `image-gen` for anything quantitative — it runs
   locally, costs no remote call, and its numbers are computed rather than
   imagined.
