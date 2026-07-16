# Public Rust API

**Implementation status:** Transitional v0.2 surface.

The workspace exposes pure domain contracts from `sfumato-domain`, application
ports and workflows from `sfumato-core`, and production implementations from
`sfumato-adapters`. The root binary is the composition and presentation layer.

## Slide Workflows

```rust
pub async fn generate_slides(
    request: GenerationRequest,
    options: GenerateSlidesOptions,
) -> anyhow::Result<GenerateSlidesResult>;

pub async fn edit_slides(
    request: EditSlidesRequest,
    options: EditSlidesOptions,
) -> anyhow::Result<EditSlidesResult>;
```

Both option DTOs receive `ProviderFactory`, `PromptCatalog`, and `ArtifactStore`
ports. This keeps connector HTTP, template resolution, and artifact persistence
replaceable in tests and alternate frontends. The CLI and TUI call the same root
execution functions and receive the same core result DTOs.

## Stable Contracts

- `ReviewableDocument` and `DeckDocument` provide revision-guarded RFC 6902
  review and editing.
- `TextModel::complete` performs exactly one provider turn.
- `AgentRunner` owns tool rounds, tool execution, and tool-exhaustion behavior.
- `PromptCatalog` resolves `PromptId` plus structured variables into text and
  `PromptProvenance`.
- `ArtifactStore` creates an `ArtifactTransaction` which commits one immutable
  `ResourceArtifactManifest` revision.
- `ProviderFactory` resolves text and image profiles without leaking connector
  details into slide workflows.

## Operation Lifecycle

`OperationContext` is the runtime-neutral lifecycle contract:

```rust
pub struct OperationContext {
    pub job_id: JobId,
    pub deadline: Option<Instant>,
    pub cancellation: CancellationToken,
    pub events: Arc<dyn EventSink>,
}
```

Its event sink is bounded and nonblocking by contract. `SfumatoError` exposes a
stable `ErrorCode`, `ErrorClass`, optional `OperationStage`, safe message, and
structured details. These types and their tests are implemented; mapping every
legacy `anyhow` workflow exit and process operation into them remains tracked in
the architecture traceability table.

## Results And Provenance

Generation results include selected models, injected tools, review/layout
summary, internal and published artifacts, warnings, and every resolved prompt's
ID, origin, version, and SHA-256 source hash. Edit results include changed slide
IDs and patch operation count. A committed `manifest.json` is the authoritative
inventory for one revision.

## Compatibility Direction

The next boundary step is a single `SfumatoApplication` facade that resolves
configuration once and accepts `OperationContext` for every call. Until that
facade replaces the free functions, modules documented above are public but not
promised as a stable semver surface.
