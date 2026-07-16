# Configuration Schema v4

**Implementation status:** Active in v0.2.

This is the normative v4 file and merge contract. Unknown fields and any schema
version other than 4 are read-only errors.

## Documents

| Scope | Default path | Purpose |
| --- | --- | --- |
| Global | `~/.config/sfumato/config.toml` | User profile, connectors, models, defaults, roles, and Marp. |
| Registry | `~/.config/sfumato/projects.toml` | Registered project roots and active project. |
| Project | `<project-root>/.sfumato/project.toml` | Portable theme, publication, model overrides, and Marp. |

Every document starts with `schema_version = 4`. Prompt templates use
conventional directories documented in [Prompt Authoring](prompt-authoring.md);
their paths are not stored in config.

## Global Example

```toml
schema_version = 4

[user]
name = "Alex"
learning_style = ["visual", "step-by-step"]

[connectors.ollama]
base_url = "http://localhost:11434/v1"

[connectors.openrouter]
base_url = "https://openrouter.ai/api/v1"
credential = "env:OPENROUTER_API_KEY"

[connectors.openrouter.headers]
HTTP-Referer = "https://example.edu"

[models.local-text]
connector = "ollama"
model = "llama3.2"
capabilities = ["text", "code"]

[models.local-text.options]
temperature = 0.4
max_tokens = 4000

[defaults]
text = "local-text"

[model_roles]
reviewer = "local-text"

[marp]
pdf = false
# browser_path = "/path/to/chromium"
```

Connector `credential`, when present, is an indirect `SecretRef`, never secret
material. v0.2 supports `env:<VARIABLE>`; the environment variable name must be
portable uppercase shell syntax. A connector with no authentication omits the
field. Other syntactically valid schemes are reserved and fail as unsupported
until an adapter explicitly implements them.

## Registry Example

```toml
schema_version = 4
active = "university"

[projects.university]
path = "/Users/alex/Notes/university"
```

Project names are validated `ProjectName` values: non-empty, at most 128 bytes,
without surrounding whitespace, control characters, path separators, `.` or
`..`. Registry paths are saved absolute and normalized.

## Project Example

```toml
schema_version = 4
name = "university"
theme = "sfumato-default"
publish_dir = "Published Slides"

[model_defaults]
text = "local-text"

[model_roles]
reviewer = "local-text"

[marp]
pdf = true
```

Model profile and theme names are lowercase portable slugs. Capabilities are
`text`, `code`, `image`, `video`, `speech`, or `embedding`. Model provider IDs
remain provider-owned strings and are not slug-normalized.

## Field Contract

- `user.name` is optional; `user.learning_style` is a non-empty string array.
- `connectors.<name>` contains `base_url`, optional indirect `credential`, and
  optional string `headers`. A connector name is a portable token.
- `models.<name>` contains connector name, provider model ID, non-empty unique
  capabilities, and typed TOML `options`.
- `defaults` maps capability to model profile.
- `model_roles` maps role to model profile; v0.2 defines `reviewer`.
- global `marp` contains `pdf` and optional `browser_path`.
- project config contains `name`, `theme`, optional `publish_dir`, optional
  `model_defaults`, optional `model_roles`, and optional `marp`.
- registry contains optional `active` and a project-name-to-path map.

Raw credentials, prompt paths, artifact workspace paths, generated job IDs, and
artifact manifests are not config fields.

## Resolution

Values resolve from lowest to highest precedence:

1. global config;
2. selected project config;
3. command request overrides.

Maps merge by key. Project or command model selection replaces the lower value
for that capability or role. Project `marp`, when present, replaces global
`marp`; a command PDF flag can only enable PDF for one operation. Command theme
replaces project theme. Publication path resolves from command, then project.

Before returning an immutable `ConfigSnapshot`, resolution validates selected
project identity, model-to-connector references, required capabilities, secret
scheme support, theme adapters, publication path semantics, and configured
browser path. One operation retains one snapshot even if files change.

## Version Reset

v0.2 deliberately does not migrate older configuration. Versions 1 through 3,
missing versions, and versions newer than 4 fail without rewriting the source.
Recreate the user document with `sfumato init user --force`, then register each
project again. Existing project files and generated artifacts are not deleted.

## Mutation

`config show --scope effective` is read-only. `set` and `delete` target global or
project scope, apply a typed dotted-key mutation in memory, validate the whole
document and cross-references, then write a temporary file beside the target,
sync it, and atomically replace the target.
Deleting a required field, inserting a raw secret, or creating an unknown field
fails without writing.
