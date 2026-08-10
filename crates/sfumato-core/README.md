# sfumato-core

Workflows, ports and the application facade for [Sfumato](https://sfumato.sh).

`SfumatoApplication` is the entry point: generating slides, documents, pages and
videos, editing a deck, and managing projects, connectors, models, themes,
templates, artifacts, prompts, plugins, tools and renderers. Everything it
touches outside itself is a port — providers, renderers, stores, prompts, secrets,
knowledge — so the same use cases can be driven by a CLI, a TUI, or a service.

Carries no infrastructure of its own: no `anyhow`, no `reqwest`, no `tokio`, no
`println!`. An architecture test enforces that.

Part of the [getsfumato/sfumato](https://github.com/getsfumato/sfumato)
workspace. Pair it with `sfumato-adapters` for a working composition, or
implement the ports yourself.
