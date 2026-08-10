# sfumato

A local-first CLI that turns one instruction plus your own material into a
finished learning resource: a Marp slide deck, a paginated document, a
self-contained interactive page, or an MP4 film.

```console
curl -fsSL https://sfumato.sh/install.sh | sh
```

or, from source:

```console
cargo install sfumato --locked
```

Then:

```console
sfumato init user
sfumato connector setup ollama
sfumato init project university --path /path/to/vault
sfumato generate slides --project university --instruction "Explain Fourier series visually"
```

Run `sfumato` with no subcommand for the interactive workspace.

It works from **your** material: pass files and directories per run, or ground a
project in a [Vitruvio](https://github.com/getsfumato/vitruvio) brain and let the
model interrogate evidence with provenance instead of browsing a directory.

Every run is kept as an immutable revision under `~/.sfumato`, and the processed
artifact can be published next to the notes it came from.

Rendering needs external tools depending on what you generate — `marp`, a
Chromium-family browser, `ffmpeg`, `node`/`npm`, `uv`. `sfumato renderer doctor`
reports what is missing.

Full documentation, architecture and guides:
[getsfumato/sfumato](https://github.com/getsfumato/sfumato).
