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

Structural templates are opt-in per generation. `--template <name>` resolves
the selected package before any provider call; omitting the flag leaves the
generation untemplated even when reusable templates are installed. Draft
prompts receive the selected structure as read-only context and require
content-only output. Sfumato inserts that output into the marker, then runs the
normal normalization, review, browser/layout inspection, artifact transaction,
and publication stages.

## Project Artifacts

The project catalog lives under `<project-root>/.sfumato/assets/`. A logical
artifact owns semantic metadata (`description`, `alt_text`, tags, and an
optional regeneration prompt) plus concrete variants keyed by theme name or
`*`. Exact-theme variants win over wildcard variants.

`artifact add` associates a file with the selected project's current theme by
default. `--theme <name>` chooses another exact association and `--all-themes`
registers a universal variant. `artifact edit` updates metadata, adds or clears
the regeneration recipe, or reassigns a legacy variant from one theme key to
another. Schema-1 manifests migrate atomically to schema 2; because their theme
cannot be inferred safely, existing files become wildcard variants.

Before a generation call, Sfumato resolves every artifact against the active
theme. A missing exact/wildcard variant is regenerated and cached when both a
recipe and an image model are available. Otherwise it is omitted with a
warning. The prompt receives purpose, alt text, tags, selected theme, media type,
and exact renderer path. Reusable artifacts do not disable `sfumato_image_gen`;
the drafter is explicitly asked to generate additional purpose-built visuals
when they improve teaching.

Generation verifies every digest but copies only files referenced by the final
reviewed document. Unreferenced catalog files and unused image-tool outputs are
removed from staging before commit. This prevents resource revisions from
accumulating unrelated media or depending on mutable external files. Active SVG
content, external URLs, traversal, and symlinks are rejected.

```bash
sfumato artifact add ./spectrum.png --name square-wave-spectrum \
  --theme gruvbox --description "Odd-harmonic square-wave spectrum" \
  --alt-text "Amplitude bars at odd harmonics" \
  --tag fourier --tag spectrum \
  --prompt "Draw the same labeled odd-harmonic spectrum and composition"

sfumato artifact edit square-wave-spectrum \
  --from-theme '*' --to-theme gruvbox \
  --prompt "Draw the same labeled odd-harmonic spectrum and composition"
```

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

Page component libraries are ordinary CDN-installed page plugins. Supported
metadata is discoverable with `sfumato plugin list`; downloaded versions live
under `~/.sfumato/plugins`, outside every Sfumato repository and executable.
The catalog records version-pinned public CDN URLs, dependency graphs, integrity
hashes, licenses, model guidance, and deterministic runtime ordering.

`sfumato plugin enable <id>` persists a project default. `--plugin <id>` (or its
`--ui` alias) adds a plugin for one request. `--shadcn` is a convenience alias
for adding the `shadcn` plugin. Dependencies such as `react` and `react-dom` are
installed and loaded first. Installation contacts the declared public CDNs;
generation itself remains offline and embeds only locally installed runtimes.

Shadcn is source-distributed rather than a browser runtime. Its plugin therefore
installs hash-pinned official registry definitions plus Tailwind's browser
compiler. Generated pages translate those accessibility and styling contracts
into semantic HTML and local JavaScript rather than emitting unusable TSX or
Radix imports.
