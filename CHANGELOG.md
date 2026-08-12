# Changelog

Notable changes. This project follows [Conventional Commits](https://www.conventionalcommits.org),
so `git log` is the complete record; this file is the readable summary.

## 0.5.0 — 2026-08-12

### Added

- **knowledge:** point one run at another brain with --brain and --brain-project
- **knowledge:** address a brain by its Vitruvio project as well as its name

## 0.4.0 — 2026-08-11

### Added

- **config:** move the browser path out of [marp] to [browser] path

### Fixed

- **ci:** install rustfmt and clippy in the release gate
- **plugins:** move the registry off master and say when it degrades
- **tests:** stop the brain stub racing its own executable bit

### Documentation

- **core:** document the last two modules, and guard the lint
- **core:** document the generation requests, outputs and review summaries
- **core:** document the configuration model
- **core:** document the provider ports
- **core:** document the theme, model and connector surfaces
- **core:** document the config-editor, setup and project surfaces
- **architecture:** draw the knowledge port, and check that diagrams parse

## 0.3.1 — 2026-08-11

### Fixed

- **renderers:** resolve external tools on PATH before spawning them
- **pages:** build file:// URLs instead of interpolating a path into one

### Documentation

- point at the published crates
- rewrite the README around installing, and follow the repo rename

## 0.3.0

The first published release. Everything before this existed only as source.

### Added

- **Installation.** `curl -fsSL https://sfumato.sh/install.sh | sh` installs a
  prebuilt binary on macOS and Linux, x86_64 and aarch64, including a statically
  linked musl build. Previously the only route was building from source.
- **`sfumato --version`.** It did not exist, which the installer noticed on its
  last line.
- **Paginated documents.** `sfumato generate document` produces a print-ready PDF
  through Paged.js, with a cover, a table of contents, and page-size selection.
  `sfumato renderer install pagedjs` provisions the renderer.
- **Vitruvio grounding.** A project can be grounded in a
  [Vitruvio](https://github.com/getsfumato/vitruvio) brain instead of the
  filesystem, so the model interrogates typed evidence with provenance rather than
  browsing a directory. Optional and off by default; see
  [ADR-0010](docs/adr/0010-knowledge-port.md).
- **`chart-gen`.** A generation tool that writes and locally executes Python to
  produce charts, for slides, documents and pages. Requires
  `--allow-code-execution` or project `security.allow_python`.
- **Connectors** for Anthropic (native Messages API), LM Studio, and ElevenLabs
  speech.
- **`sfumato theme regenerate`**, which re-derives a theme's renderer stylesheets
  from its manifest, and **`sfumato connector presets`**, which lists what
  `connector setup` accepts.
- **`--no-pdf`** on `generate slides`. Configuration could turn PDF export on but
  never off for a single run.
- **Published library crates.** `sfumato-core`, `sfumato-adapters` and
  `sfumato-domain` are published so another front end can reuse the workflows
  without this repository. `ApplicationRoots` lets a caller say where
  configuration and data live instead of inheriting the process user's home.
- **CI**, on Linux and macOS, plus an MSRV job, crates.io packaging checks, and a
  guard against enabling the opt-in test suites by accident.

### Changed

- **Browser discovery works off macOS.** It probed three hardcoded
  `/Applications` paths and nothing else, so on Linux it always failed — taking
  page inspection, document measurement and Marp layout inspection with it.
  Resolution is now the configured path, then `SFUMATO_BROWSER` /
  `PUPPETEER_EXECUTABLE_PATH` / `CHROME_PATH`, then `PATH`, then per-platform
  well-known locations.
- **The TUI** was rebuilt: thirteen destinations in three groups, a `Ctrl+K` jump
  palette, a `?` key reference, filterable pickers instead of typed identifiers, a
  progress line with an activity feed, and a quit confirmation that requires `y`.
  It also generates documents and reaches tools and plugins, which had no entry at
  all.
- **`marp.browser_path`** is documented, and a configured path that does not exist
  is now reported rather than silently ignored. The setting is not Marp-only —
  pages, documents and diagrams use the same browser — and will move in a future
  schema version.
- **Minimum supported Rust is 1.91**, which is what the code actually requires;
  edition 2024 alone would suggest 1.85.
- The binary moved to `cli/` in the workspace, making the repository root a virtual
  manifest. No effect on the installed command.

### Fixed

- `--disable-plugin` could not reach the UI library, and the TUI could not reach
  video options.
- Accents are folded when comparing titles and deriving asset names.
- Every design token is emitted as a custom property.
- Scripted SVG is accepted in pages, and every remote reference is named.
- Progress events honour `NO_COLOR`.
- The TUI's dimmest text is legible, and the exit prompt is sized to its content.
