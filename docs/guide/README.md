# Sfumato CLI Guide

This guide is the authoritative operational manual for humans and automation
agents using the Sfumato CLI. It documents command syntax, resolution rules,
side effects, managed files, publication layouts, and failure behavior.

## Reading Order

| Document | Use it for |
| --- | --- |
| [Getting started](01-getting-started.md) | Installation, first configuration, and first generated resource. |
| [Configuration and projects](02-configuration-and-projects.md) | Schema v5, scopes, project registry, precedence, and `SFUMATO.md`. |
| [Connectors and models](03-connectors-and-models.md) | Ollama, OpenRouter, Codex, credentials, capabilities, profiles, and defaults. |
| [Resource building blocks](04-resource-building-blocks.md) | Themes, templates, artifacts, prompts, page plugins, generation tools, and video renderers. |
| [Generate slides](05-generate-slides.md) | Marp generation, review, Mermaid, MathJax, images, PDF, and publication. |
| [Generate pages](06-generate-pages.md) | Standalone HTML, UI libraries, utilities, offline runtimes, browser review, and publication. |
| [Generate videos](07-generate-videos.md) | Hyperframes, Manim, remote video models, options, repair, inspection, and publication. |
| [Generate documents](12-generate-documents.md) | Paginated PDF prose: sections, cover, contents, page setup, format repair, and publication. |
| [Editing, TUI, and automation](08-editing-tui-and-automation.md) | Focused deck editing, interactive mode, JSON mode, progress events, and agent usage. |
| [Command reference](09-command-reference.md) | Every command, positional argument, flag, accepted value, and immediate effect. |
| [Troubleshooting](10-troubleshooting.md) | Configuration, provider, browser, Marp, Mermaid, plugin, Hyperframes, and Manim failures. |
| [Hyperframe troubleshooting](11-hyperframe-troubleshooting.md) | Timeline contracts, local assets, snapshots, managed review sessions, and safe reproduction packages. |

## CLI Conventions

- `sfumato` with no arguments opens the TUI only when the process is attached to
  an interactive terminal.
- `sfumato <group> <command>` executes a non-interactive Clap command.
- Optional `[INPUTS]...` are local textual files or directories. Directories are
  recursively discovered using the supported extension allowlist.
- `--project <name>` selects a registered project for one command. Without it,
  Sfumato uses the globally active project.
- Repeatable flags are shown more than once, for example `--plugin motion
  --plugin threejs` or `--model text=codex --model code=codex`.
- Paths containing spaces must be shell-quoted.
- Generation instructions are always required and should describe the teaching
  objective, audience, depth, language, and desired emphasis.
- `--dry-run` resolves configuration and renders prompts without invoking models,
  renderers, browsers, or artifact writes.
- `--json` reserves stdout for machine-readable results. Human progress is
  suppressed so agents can parse the response reliably.
- Commands return a non-zero exit status on validation, configuration, provider,
  rendering, or publication errors unless the workflow explicitly treats the
  condition as a warning.

## Core Concepts

| Concept | Meaning |
| --- | --- |
| User profile | One global name and learning-style profile. |
| Project | A registered source root such as an Obsidian vault, with portable `.sfumato` settings. |
| Connector | Connection and authentication mechanism for a provider or local model runtime. |
| Model profile | Named connector/model pair with declared capabilities and typed options. |
| Theme | Reusable semantic design tokens plus Marp and HTML adapters. |
| Template | Optional reusable page or slide structure containing one content marker. |
| Project artifact | Reusable logical image with metadata and theme-specific variants. |
| Page plugin | Installed UI library, utility library, or internal browser runtime. |
| Generation tool | Optional model-facing function — `image-gen`, `video-gen`, `audio-gen`, or the locally executed `chart-gen`. |
| Renderer | Explicitly installed local Hyperframes or Manim executable environment. |
| Managed artifact | Immutable resource revision committed under `~/.sfumato/Projects`. |
| Published artifact | Processed PDF, HTML tree, or MP4 copied into a user-selected directory. |

## Agent Checklist

Before generation, an agent should:

1. Run `sfumato project show [name]` to confirm the project root.
2. Run `sfumato config show --scope effective --project <name>` to inspect the
   selected theme, defaults, tools, page settings, and publication directory.
3. Run `sfumato model list` and confirm every required capability has a profile.
4. For pages, run `sfumato plugin list --project <name>`.
5. For local video, run `sfumato renderer doctor <renderer>`.
6. Run the intended generation command once with `--dry-run` when model or tool
   selection is uncertain.
7. Use `--json` for the real call and consume paths from the returned object
   rather than guessing artifact locations.
