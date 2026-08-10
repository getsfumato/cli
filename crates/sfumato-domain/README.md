# sfumato-domain

Pure domain types and review rules for [Sfumato](https://sfumato.sh) — decks,
documents, pages, video plans, artifact manifests, and the RFC 6902 review
contract they share.

No filesystem, no network, no process, no async: those are enforced by an
architecture test rather than left to discipline. Depends on nothing else in the
workspace.

Part of the [getsfumato/sfumato](https://github.com/getsfumato/sfumato)
workspace, alongside `sfumato-core` (workflows and ports), `sfumato-adapters`
(concrete infrastructure), and the `sfumato` CLI.
