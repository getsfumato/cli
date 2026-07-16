# ADR-0006: Commit Immutable Resource Revisions

**Decision status:** Accepted and implemented for v0.2.

## Context

A deck may include Markdown, PDF, theme CSS, generated images, Mermaid sources,
rendered diagrams, and a published PDF. Rendering or cancellation can occur
after several outputs exist. Publishing those files directly would expose
partial or stale resources.

## Decision

Use an `ArtifactTransaction` for generation and edit:

1. acquire the project artifact lock;
2. create a unique same-filesystem staging directory for the job;
3. render and validate every candidate inside staging;
4. validate the complete manifest, relative paths, declared files, and symlink containment;
5. write and sync `manifest.json` inside staging;
6. rename the entire staging directory to a new immutable revision directory;
7. atomically replace the resource's `current.json` pointer.

Because an existing revision is never modified, commit requires one directory
rename rather than a sequence of replacements and backups. Dropping an
uncommitted transaction removes staging. Edit creates a child revision and
records its parent revision in the manifest.

Publication occurs after workspace commit and includes processed artifacts
only. It uses a destination-local temporary file and atomic rename. Publication
failure returns a warning and never rolls back a valid workspace revision.

## Consequences

- Failed generation and edit do not expose candidate current files.
- Existing revisions provide history without backup restoration.
- `current.json` is the only mutable resource pointer.
- Staging and immutable history require additional disk space.
- Crash cleanup for abandoned pre-commit staging directories remains an
  operational maintenance concern; committed revisions need no journal replay.
