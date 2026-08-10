# ADR 0010: Ground A Project In A Brain Behind A Knowledge Port

## Status

Accepted.

## Decision

Grounding material reaches a generation through a `BrainClient` port with two
backends, selected per project by `knowledge.backend`.

`filesystem` is the default and unchanged: the model browses the sources handed to
the run through read-only tools scoped to the project and its inputs.

`vitruvio` replaces that with retrieval. The prompt carries an inventory of the
brain — its five memory modules, block counts, Merkle roots, facets and travelling
indices — instead of a file index, and one tool, `sfumato_search_brain`, replaces
`sfumato_list_directory` and `sfumato_read_file`. Matches come back as evidence
with provenance.

The adapter shells out to the `vitruvio` CLI rather than speaking to a service,
because Vitruvio exposes no network surface: a brain is a local directory and the
CLI is the supported way in. Its `--json` contract is one object on stdout with
stable error and exit codes, which is narrow enough to build on. `--actor-kind` is
always `agent`: a brain that records who asked should not be told a human did.

Source paths are **refused** under a brain, not ignored. Accepting them while
reading from the brain would silently answer a different question than the one
asked.

Degradation is asymmetric on purpose. A brain that cannot report index statistics
still yields a usable card, so that is a warning; a brain that cannot be reached
at all fails the run. `EvidenceMatch::score` stays a string because it is neither
a probability nor a confidence, and typing it as a float would invite arithmetic
that means nothing.

The compacted retry that follows a context-limit failure has no brain access, so
it replays the `BrainEvidenceRecord`s already retrieved. Building that record
became lazy for this reason.

## Consequences

A project can be grounded in portable, verifiable knowledge without Sfumato owning
an index, an embedding model, or a query planner — Vitruvio owns those, and it is
interchangeable behind the port. The `filesystem` backend stays the default, so
nothing about an existing project changes and no user acquires a dependency they
did not ask for.

Retrieval modes (`auto`, `exact`, `lexical`, `semantic`, `associative`) pass
through uninterpreted. Sfumato does not model Vitruvio's planner, so a new mode
needs no change here.

The cost is a second grounding path through every generation workflow, and prompt
partials that must render for both. That is why the choice is per project rather
than per run: one project answers the question once.

A brain is resolved per invocation, so one client instance serves every project. A
transport needing a session would make this a factory instead.
