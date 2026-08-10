# sfumato-adapters

Concrete implementations of the [`sfumato-core`](https://crates.io/crates/sfumato-core)
ports for [Sfumato](https://sfumato.sh).

Model providers (Ollama, LM Studio, OpenRouter, Anthropic, the local Codex app
server, ElevenLabs), renderers (Marp, Mermaid, Paged.js, Hyperframes, Manim),
config and artifact stores, prompt resolution, secrets, page plugins, managed
Python environments, and the Vitruvio knowledge client.

`application::production_application()` is the composition root: it wires every
adapter and hands back a ready `SfumatoApplication`.

Prompts, themes and runtimes are compiled into the binary, so nothing is fetched
or read from a package directory at runtime.

Part of the [getsfumato/sfumato](https://github.com/getsfumato/sfumato) workspace.
