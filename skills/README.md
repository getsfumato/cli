# Skills

The agent-facing contracts for the `sfumato` CLI. Everything here is written for
a model driving Sfumato rather than for a person reading a manual: what to run,
which flag decides the outcome, and how to read a result without over-claiming.

Copy the directories into a repository's `.claude/skills/` — nothing is generated
at install time.

| Skill | Covers |
|---|---|
| `sfumato` | The entry point: the four resources, the five things a run resolves, the three config layers, the artifact store, `--json`. |
| `sfumato-cli` | The command surface: every group, every command, the flag that decides the outcome. |
| `sfumato-generate` | The four resources in depth — pipelines, flags, renderers, failure modes. |
| `sfumato-setup` | Zero to a first generation: init, connectors, model profiles, credentials, renderers. |
| `sfumato-library` | Themes, structural templates, project artifacts, prompt overrides, page plugins, generation tools. |
| `sfumato-knowledge` | Filesystem grounding versus a Vitruvio brain. |
| `sfumato-automation` | Driving Sfumato from a script or an agent. |

`sfumato` carries three references: [`cli-reference.md`](sfumato/references/cli-reference.md),
[`error-contract.md`](sfumato/references/error-contract.md), and
[`artifact-store.md`](sfumato/references/artifact-store.md).

## Layout

Each directory holds a `SKILL.md` opening with YAML frontmatter:

```yaml
---
name: sfumato-cli
description: >-
  One or two sentences saying what this covers *and when to reach for it*. This is
  what decides whether the skill loads at all, so a vague one makes the skill dead
  weight.
allowed-tools: Bash(sfumato:*), Read
---
```

Two things a tool installing these should know:

- **The set is layered, not independent.** `sfumato` is the entry point and the
  others are reached from it. Installing one alone works but loses the routing.
- **Cross-references are relative.** Every skill links to
  `../sfumato/references/…`, so install `sfumato` alongside anything else or
  those links dangle.

## One of these is checked, not trusted

`sfumato/references/cli-reference.md` is **generated** from the clap command
declarations by the `renders_the_committed_cli_reference` test in
`cli/tests/unit/cli.rs`, and `cargo test` fails when the committed copy is stale.

```console
SFUMATO_WRITE_CLI_REFERENCE=1 cargo test -p sfumato cli_reference
```

Generated from the parser rather than written by hand for the reason the file
itself states: a reference that disagrees with the parser is worse than none,
because it costs a turn to discover which of the two is lying. Adding a command
or a flag now forces the reference to be regenerated in the same change.

The prose in the other skills is not machine-checked. When you add a command,
`cli-reference.md` updates itself but `sfumato-cli/SKILL.md` will not — its
tables name the flag that decides the outcome, which is a judgement no generator
can make.

## What they all teach

1. `--project` explicitly, `--json` always, exit status before parsing.
2. `--dry-run` before the first run against a new project. It is free.
3. **Read `warnings` even when the status is zero.** A deck without its PDF, a
   page with overflow, and a film that grew longer are all successes with a
   warning, and all three look clean if you only check the status.
4. `error.stage` says where the pipeline stopped, and `error.class` says whether
   trying again can possibly help — "retryable" never means "repeat the same
   call".
5. Take paths from the result. A revision directory constructed by hand is wrong
   the next time anything succeeds.
