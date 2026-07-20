# Public Rust API

**Implementation status:** active v0.2 facade.

External frontends use `sfumato_core::application::SfumatoApplication`. Production
composition is provided by `sfumato_adapters::application::production_application`.
Raw slide orchestrators and their dependency bundles are crate-private.

## Application Facade

```rust
let application = sfumato_adapters::application::production_application()?;

let result = application
    .generate_slides(GenerateSlidesCommand {
        operation,
        config: ConfigOverrides::default(),
        request,
        title: None,
        template: None,
        dry_run: false,
        review: true,
        event_sink: None,
    })
    .await?;
```

`generate_slides` returns `SfumatoResult<GenerateSlidesResult>` and
`generate_page` returns `SfumatoResult<GeneratePageResult>` and `generate_video`
returns `SfumatoResult<GenerateVideoResult>`. Page commands carry
generic installed plugin IDs and return the resolved versions and runtime hashes.
`edit_slides` returns `SfumatoResult<EditSlidesResult>`. The same facade also
owns project, model, connector, theme, prompt, setup, and configuration use
cases. CLI and TUI construct the same command DTOs.

## Outbound Ports

`SfumatoApplicationDependencies` makes external variability explicit:

- `EffectiveConfigResolver` and `PromptCatalogFactory`;
- `PromptManager` and schema-aware `ConfigEditor`;
- `ArtifactStore`, repositories, and `WorkspaceFileSystem`;
- `ProviderFactory`, `ConnectorIntrospection`, `TextModel`,
  `ImageGenerationProvider`, and `VideoGenerationProvider`;
- `SecretResolver` for provider authentication and `SecretStore` for secure
  connector login, status, and logout workflows;
- `DiagramRenderer`, `SlideRenderer`, `VideoRenderer`, `RendererManager`,
  `SourceReader`, and `GenerationToolFactory`.
- `PageAssembler`, `PageInspector`, and `PagePluginCatalog` for standalone pages.
- `GenerationTemplateCatalog` and `ProjectAssetCatalog` for reusable structure
  and portable project media.

Every core port returns a typed result. Adapters may use implementation-specific
diagnostics internally, but classify and sanitize failures before crossing the
port boundary.

## Stable Contracts

- `ReviewableDocument` and `DeckDocument` expose revision-guarded RFC 6902
  snapshots and transactional patch application.
- `PageDocument` exposes the same revision-guarded contract over `title`,
  `body_html`, `css`, and `javascript`; browser repair cannot change its title.
- `VideoPlanDocument` guards engine-neutral plans; `VideoSourceDocument` guards
  focused RFC 6902 changes to Hyperframes or Manim source files.
- `TextModel::complete` performs one provider turn only for request/response
  transports such as OpenAI-compatible APIs.
- `CodexAppServerProvider` owns a persistent JSON-RPC App Server process. It
  discovers authenticated models through `model/list`, starts ephemeral
  read-only threads, executes Sfumato `dynamicTools` through `item/tool/call`,
  and consumes streamed item and turn lifecycle events.
- `ConnectorIntrospection` reports `ConnectorCapabilities` before dispatching
  native model-catalog and status operations. OpenRouter and Ollama adapters
  compose `OpenAiCompatibleConnector`; Codex dispatches directly to App Server.
- `AgentRunner` owns transcripts, tools, tool-round limits, the final no-tools
  output-contract turn, and cancellation checkpoints for transports that expose
  isolated model turns. Codex App Server owns its native turn loop instead.
- `PromptCatalog` resolves a stable `PromptId` and `PromptVariables` into a
  rendered message plus `PromptProvenance`.
- `ArtifactTransaction` stages files and commits one immutable
  `ResourceArtifactManifest` revision.
- `OperationContext` carries `JobId`, cancellation, optional deadline, and a
  bounded nonblocking `EventSink`.
- `SecretValue` requires explicit exposure at the transport boundary, while
  `SecretRef` persists only an indirect `stored:` or `env:` locator.

## Errors

`SfumatoError` is the only core application error:

```rust
pub struct SfumatoError {
    pub code: ErrorCode,
    pub class: ErrorClass,
    pub retryable: bool,
    pub stage: Option<OperationStage>,
    pub message: String,
    pub details: BTreeMap<String, String>,
}
```

Classes distinguish retry, context compaction, invalid-output repair,
dependency unavailability, cancellation, and permanent failure. Provider limits,
tool failures, renderers, artifact persistence, prompt rendering, and local
validation retain their classification through the facade. Messages and details
are sanitized before presentation.

## Results And Provenance

Generation results identify the selected project and model profiles, declared
tools, structural template, actually referenced project-artifact variants,
themes, metadata, hashes, and paths,
committed and published paths, review/layout state, warnings, project
instruction path, and every contributing prompt's ID, origin, version, and
SHA-256 hash. Page results also report automatically embedded runtimes such as
MathJax with their pinned version and integrity hash. Edit results additionally report changed slide IDs, patch count,
context compaction, and parent-linked revision artifacts.

The committed `manifest.json` and `current.json` pointer are authoritative.
Published PDFs, HTML pages, and MP4 files are convenience copies and never supersede the
managed revision. Page publication uses
`<out>/_sfumato/pages/<slug>/{index.md,index.html,assets/}` so generated content
is explicit and navigable from Obsidian. Video publication writes only
`<out>/_sfumato/videos/<slug>/<slug>.mp4`; plans and generated source remain in
the managed immutable revision.

## Public Surface Policy

`#![warn(missing_docs)]` is enabled in domain, core, and adapters. The facade,
ports, errors, operations, prompts, artifacts, and curated adapter constructors
are documented public API. Broad DTO/service modules currently use scoped
`#[allow(missing_docs)]` declarations while they are narrowed; this does not
relax documentation linting for the curated integration surface.
