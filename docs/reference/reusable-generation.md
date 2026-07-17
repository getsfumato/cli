# Reusable Generation Inputs

Sfumato keeps visual style, structural composition, and reusable media as
independent inputs:

| Concept | Scope | Responsibility |
| --- | --- | --- |
| Theme | User-global, selected by project | Semantic tokens and renderer adapters |
| Template | User-global, selected per command | Reusable page or deck structure |
| Project artifact | Portable project-local catalog | Logos, icons, and common visual media |
| Prompt | Layered project/user/bundled | Model-facing generation policy |

## Structural Templates

Packages live at `~/.config/sfumato/templates/<name>/` and contain
`template.toml` plus either `template.md` or `template.html`. The source must
contain exactly one `<!-- SFUMATO_CONTENT -->` marker. Names use lowercase
letters, digits, and hyphens; source paths cannot be absolute or traverse out
of the package.

`--template <name>` resolves the package before any provider call. Draft
prompts receive the structure as read-only context and require content-only
output. Sfumato inserts that output into the marker, then runs the normal
normalization, review, browser/layout inspection, artifact transaction, and
publication stages.

## Project Artifacts

The project catalog lives under `<project-root>/.sfumato/assets/`. Adding a file
copies it into the managed `files/` directory and records its media type,
description, original filename, and SHA-256 digest in `manifest.toml`.

Generation verifies every digest, copies assets into its private transaction,
and gives the model an exact renderer-relative path. This prevents a committed
resource from depending on a mutable external file. Active SVG content,
external URLs, traversal, and symlinks are rejected.

## DESIGN.md Exchange

`sfumato theme import` and `sfumato theme export` implement the alpha
[Google DESIGN.md format](https://github.com/google-labs-code/design.md/blob/main/docs/spec.md).
Normative YAML colors become semantic theme colors. Recognized heading/body
typography becomes Sfumato font tokens. The imported DESIGN.md remains in the
theme package as its human-readable source, while generated Marp and HTML
adapters make the theme immediately usable.

Unknown Markdown prose is preserved on import. Invalid colors, unsupported
versions, duplicate canonical sections, and unsafe output paths fail without
creating a usable theme.

## React Component Libraries

Page component libraries are ordinary manifest-driven page plugins. The
catalog supports dependency graphs, integrity hashes, licenses, model guidance,
and deterministic runtime ordering. `--ui <id>` aliases `--plugin <id>`.

The bundled `materialui` entry resolves `react` and `react-dom` automatically.
All runtimes are pinned and embedded before generated page JavaScript. Generated
code uses browser globals and `React.createElement`; Sfumato never invokes npm,
downloads a CDN asset, or emits a multi-file React build during generation.
