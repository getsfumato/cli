---
name: sfumato-knowledge
description: Where a sfumato resource's claims may come from — local files or a Vitruvio knowledge brain. Use when grounding a project in a brain, when source paths were refused, when configuring knowledge.project or knowledge.brain, when pointing one run at a different brain, or when a brain-backed generation failed.
allowed-tools: Bash(sfumato:*), Bash(vitruvio:*), Read
---

# Grounding: files or a brain

One project-scoped decision, `knowledge.backend`, answers one question: where may
this project's resources draw their claims from? Everything downstream —
drafting, review, diagrams, layout, rendering, publishing — is deliberately
identical either way.

| Backend | The model gets | Sources |
|---|---|---|
| `filesystem` (default) | `sfumato_list_directory`, `sfumato_read_file` | Paths you pass per run. |
| `vitruvio` | `sfumato_search_brain` | A brain. Paths are **refused**. |

## Filesystem

The default, and what every project did before the setting existed. The prompt
carries an index of the supplied files — the tree, each with its size — and the
model opens what it needs. Limits and the reasoning are in
[`sfumato-generate`](../sfumato-generate/SKILL.md).

## A Vitruvio brain

[Vitruvio](https://github.com/getsfumato/vitruvio) is a separate project
(`pip install vitruvio`). A brain is a local directory of typed,
content-addressed blocks with provenance; it returns **evidence, never prose**.
Sfumato drives its CLI, so there is no service to run. Entirely optional, off by
default.

```toml
# .sfumato/project.toml
[knowledge]
backend = "vitruvio"
project = "facultad"        # registered Vitruvio project
brain   = "simulacion"      # brain within it
```

### Two names, because Vitruvio resolves in two steps

Which project, then which brain within it — a brain name means nothing until you
know whose vocabulary it belongs to. Sfumato states **both** on every
invocation:

```console
vitruvio --project facultad --brain simulacion --actor-kind agent --json query search "..."
```

That is deliberate. Vitruvio would otherwise fall back to a walk-up from the
working directory or to a selection some earlier `vitruvio brain use` saved on
the machine. Both make what a run reads depend on something outside the project
file, and a generation that quietly drew from the wrong subject looks entirely
correct.

`knowledge.project` needs the project registered once:

```console
vitruvio project register        # in the project's directory
vitruvio project list            # names --project accepts
```

For a Vitruvio project nobody registered, point at its file instead. The two keys
are **alternatives** — Vitruvio honours a config file over a project name, so
naming both is refused rather than leaving the project key inert:

```toml
[knowledge]
backend = "vitruvio"
config  = "../vitruvio/vitruvio.toml"
brain   = "simulacion"
```

Naming neither is allowed and means Vitruvio walks up from wherever Sfumato ran.
Fine for a brain inside the project's own tree, misleading everywhere else.

### Every key

| Key | Meaning |
|---|---|
| `backend` | `filesystem` (default) or `vitruvio`. |
| `project` | Registered Vitruvio project. Excludes `config`. |
| `brain` | Required under `vitruvio`. A name the project declares, or a path. |
| `config` | An explicit `vitruvio.toml`. Excludes `project`. |
| `executable` | Path to `vitruvio` when it is not on `PATH`. `~` is expanded. |
| `actor` | Identity recorded against each query. |
| `memory_types` | Default module filter. |
| `include_superseded` | Whether replaced blocks come back by default. |
| `default_limit` | Matches when the model asks for no number. Default 10. |
| `max_limit` | Ceiling on what the model may ask for. Default 50, cap 200. |
| `timeout_seconds` | Bound on one invocation. Default 60. |

Sfumato always passes `--actor-kind agent`: a brain that records who asked should
not be told a human did.

### Per-run overrides

`--brain` and `--brain-project` point one generation somewhere else, through the
same layer as `--theme`:

```console
sfumato generate slides --brain simulacion --instruction "..."
sfumato generate slides --brain-project ethicompass --brain metrica-a --instruction "..."
```

Careful with the two `project` words: `--project` is the **Sfumato** project,
`--brain-project` the **Vitruvio** one. Unrelated registries; one command often
states both.

`--brain-project` also clears `config` for that run, since the two are
alternatives and the file would otherwise win.

Neither flag will **ground** a project that is not grounded. On `filesystem` they
are refused rather than switching the backend — grounding decides where every
claim may come from, and its side effect is refusing the source paths the command
was called with. A run may point somewhere else; only the project may change what
kind of place that is.

## What changes under a brain

Deliberately little. The model is offered one search tool instead of two file
tools, and the prompt carries an inventory of the brain — its memory modules,
block counts, and which columns can be filtered on — instead of a file index.

**Source paths are refused, not ignored.** A silently dropped path would leave
you believing a file grounded the resource when nothing did, and the resource
looks the same either way.

The search tool takes a question plus optional `memory_types`, `subject`, `tags`,
a time window, a retrieval `mode`, and a `limit` the project caps. The five
modules are `canonical` (evidence as registered), `episodic` (what happened, and
when), `semantic` (facts and claims), `procedural` (goals and steps), and
`provenance` (who derived what from what).

The prompt asks for **several distinct questions**, not one, varying the filters
rather than only rewording — a single search answers the question you already
knew how to ask. A brain-grounded run looks like this:

```text
Ask the brain  how does Jacobi iteration converge
  7 matches, 7 verified
Ask the brain  where does Jacobi fail to converge
  4 matches, 4 verified, truncated
Ask the brain  a worked Jacobi example
  3 matches, 3 verified
```

## Reading evidence without over-claiming

These rules are in the prompt, and they are the ones to check when reviewing what
a brain-grounded resource actually says:

- **`verified: false` or `resolvable: false` is not evidence.** Not quotable, not
  paraphrasable. The topic is left out instead.
- **`superseded_by` means out of date.** Cite the successor, or say explicitly
  that you are quoting a superseded claim.
- **`truncated: true` means there was more.** Never conclude the brain lacks
  something without checking it.
- **`score` is agreement between retrieval strategies** — not a probability, not
  a confidence. It is a string on purpose. Never shown, never used to rank a
  claim in the text.
- **Say nothing the brain did not return.** There is no filesystem to fall back
  on, and inventing around a gap is worse than a shorter resource.

The compacted retry after a context limit has no brain access, so it replays the
evidence already retrieved — minus anything unverified or superseded, which was
never writable from anyway.

## Failure modes

| Symptom | Cause |
|---|---|
| "not grounded in a brain, so there is no brain to override" | `--brain` on a `filesystem` project. Set `knowledge.backend`. |
| "knowledge.project and knowledge.config both name a Vitruvio project" | Keep one. |
| `PROJECT_NOT_KNOWN` | `vitruvio project register` in that project's directory. |
| "The brain tool 'vitruvio' was not found" | Install it, or set `knowledge.executable`. |
| A config error naming `knowledge.*` | Vitruvio exit 3. The message names the keys. |
| Source paths rejected | Expected under a brain. Remove them. |
| The card says a brain has no indices | Warning, not failure — statistics are an enrichment. `vitruvio index build`. |

Degradation is asymmetric on purpose: a brain that cannot report index
statistics still yields a usable inventory, so that is a warning; a brain that
cannot be reached at all fails the run.
