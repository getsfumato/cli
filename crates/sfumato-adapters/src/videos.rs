//! Managed Hyperframe/Manim installation, rendering, and MP4 inspection.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    generation::VideoFrameMeasurement,
    operation::OperationContext,
    python::PythonRuntime,
    renderers::{
        RendererManager, RendererStatus, VideoCatalog, VideoCatalogItem, VideoCatalogKind,
        VideoEngine, VideoInspection, VideoRenderRequest, VideoRenderer,
    },
};
use tokio::process::Command;
use walkdir::WalkDir;

use crate::{python::UvPythonRuntime, runtime::run_command};

const RENDERER_MANIFEST: &str = include_str!("../assets/video-renderers/manifest.toml");
const HYPERFRAME_CATALOG: &str = include_str!("../assets/video-catalog/manifest.json");
const REQUIRED_HYPERFRAMES_CHECKS: &[&str] = &["Node.js", "FFmpeg", "FFprobe", "Chrome"];
const ADVISORY_HYPERFRAMES_CHECKS: &[&str] = &["Version"];

#[derive(Deserialize)]
struct ManagedRendererManifest {
    schema_version: u32,
    renderers: BTreeMap<String, ManagedRendererPackage>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManagedRendererPackage {
    package: String,
    version: String,
    #[serde(default)]
    runtime_packages: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct HyperframesDoctorReport {
    checks: Vec<HyperframesDoctorCheck>,
}

#[derive(Deserialize)]
struct HyperframesDoctorCheck {
    name: String,
    ok: bool,
    detail: String,
}

fn evaluate_hyperframes_doctor(report: HyperframesDoctorReport) -> (bool, Vec<String>) {
    let mut healthy = true;
    let mut details = Vec::new();

    for required in REQUIRED_HYPERFRAMES_CHECKS {
        match report.checks.iter().find(|check| check.name == *required) {
            Some(check) if check.ok => {}
            Some(check) => {
                healthy = false;
                details.push(format!(
                    "required {} unavailable: {}",
                    check.name, check.detail
                ));
            }
            None => {
                healthy = false;
                details.push(format!("required {required} check was not reported"));
            }
        }
    }

    let optional = report
        .checks
        .iter()
        .filter(|check| {
            !check.ok
                && !REQUIRED_HYPERFRAMES_CHECKS.contains(&check.name.as_str())
                && !ADVISORY_HYPERFRAMES_CHECKS.contains(&check.name.as_str())
        })
        .map(|check| check.name.as_str())
        .collect::<Vec<_>>();
    if !optional.is_empty() {
        details.push(format!(
            "optional capabilities unavailable: {}",
            optional.join(", ")
        ));
    }

    (healthy, details)
}

fn renderer_package(id: &str) -> Result<ManagedRendererPackage> {
    let manifest: ManagedRendererManifest =
        toml::from_str(RENDERER_MANIFEST).context("Managed renderer manifest is invalid")?;
    if manifest.schema_version != 1 {
        bail!(
            "Unsupported managed renderer manifest schema {}",
            manifest.schema_version
        );
    }
    manifest
        .renderers
        .get(id)
        .cloned()
        .with_context(|| format!("Unknown renderer '{id}'. Use hyperframe, manim, or pagedjs."))
}

fn collect_pngs(root: &Path) -> std::collections::BTreeSet<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("png"))
        })
        .collect()
}

/// The pinned runtime every staged catalog item is rewritten to load.
const VENDORED_GSAP: &str = "vendor/gsap.min.js";
const CDN_GSAP_PREFIX: &str = "https://cdn.jsdelivr.net/npm/gsap@";
const REMOTE_FONT_HOSTS: &[&str] = &["fonts.googleapis.com", "fonts.gstatic.com"];
/// Namespace declarations are identifiers, not resources a render fetches.
const XML_NAMESPACES: &[&str] = &[
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1999/xlink",
    "http://www.w3.org/1999/xhtml",
];

/// Where a remote reference ends, given HTML attribute and CSS url() syntax.
fn reference_end(tail: &str) -> usize {
    tail.find(|value: char| value.is_whitespace() || matches!(value, '"' | '\'' | '>' | ')'))
        .unwrap_or(tail.len())
}

/// Points every CDN GSAP reference at the pinned managed runtime instead.
///
/// The registry pins its own GSAP release, so leaving the CDN URL in place would
/// both require the network and run a second GSAP beside the one Sfumato pins.
fn rewrite_cdn_gsap(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find(CDN_GSAP_PREFIX) {
        output.push_str(&rest[..start]);
        output.push_str(VENDORED_GSAP);
        let tail = &rest[start..];
        rest = &tail[reference_end(tail)..];
    }
    output.push_str(rest);
    output
}

/// Drops every `<link>` element that reaches a remote font host.
///
/// The item falls back to the theme's own fonts, which is a visual compromise;
/// a render that silently depends on the network is a correctness one.
fn strip_remote_font_links(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("<link") {
        let tail = &rest[start..];
        let Some(offset) = tail.find('>') else { break };
        let element = &tail[..=offset];
        if REMOTE_FONT_HOSTS.iter().any(|host| element.contains(host)) {
            output.push_str(&rest[..start]);
        } else {
            output.push_str(&rest[..start + offset + 1]);
        }
        rest = &tail[offset + 1..];
    }
    output.push_str(rest);
    output
}

/// Drops every CSS `@import` statement that reaches a remote font host.
///
/// Items declare their fonts either as a `<link>` or as an `@import` inside
/// `<style>`, so stripping only the markup form leaves half the catalog online.
fn strip_remote_font_imports(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("@import") {
        let tail = &rest[start..];
        let offset = tail.find(';').unwrap_or(tail.len() - 1);
        let statement = &tail[..=offset];
        if REMOTE_FONT_HOSTS
            .iter()
            .any(|host| statement.contains(host))
        {
            output.push_str(&rest[..start]);
        } else {
            output.push_str(&rest[..start + offset + 1]);
        }
        rest = &tail[offset + 1..];
    }
    output.push_str(rest);
    output
}

/// Reports the first reference a deterministic offline render could not fetch.
fn first_remote_reference(content: &str) -> Option<&str> {
    let mut rest = content;
    while let Some(start) = rest.find("http") {
        let tail = &rest[start..];
        if !(tail.starts_with("http://") || tail.starts_with("https://")) {
            rest = &tail["http".len()..];
            continue;
        }
        let end = reference_end(tail);
        let reference = &tail[..end];
        if !XML_NAMESPACES.contains(&reference) {
            return Some(reference);
        }
        rest = &tail[end..];
    }
    None
}

/// Makes one installed catalog item safe for a deterministic offline render.
///
/// The registry ships items as standalone documents that pull GSAP from a CDN
/// and fonts from Google. Core validation holds the model's own files to an
/// offline contract but never sees these, because they are staged after the
/// model has answered, so the rewrite and the guarantee both belong here. A
/// reference that survives is fatal rather than a warning: silently rendering
/// against the network is exactly the outcome the contract exists to prevent.
fn offline_catalog_item(id: &str, content: &str) -> Result<String> {
    let sanitized = rewrite_cdn_gsap(&strip_remote_font_imports(&strip_remote_font_links(
        content,
    )));
    if let Some(reference) = first_remote_reference(&sanitized) {
        bail!(
            "Managed catalog item '{id}' references '{reference}', which a deterministic offline render cannot fetch. Vendor that dependency or drop the item from the curated catalog."
        );
    }
    // A URL can be rewritten; a runtime request cannot. The caption styles were
    // dropped from the curated catalog for exactly this: they load their words
    // from a sidecar file with per-word timings, which only a narration track can
    // supply and this engine is silent. Re-adding one has to fail here rather
    // than at render time, where it reads as an unexplained renderer failure.
    for request in ["fetch(", "XMLHttpRequest", "WebSocket("] {
        if sanitized.contains(request) {
            bail!(
                "Managed catalog item '{id}' performs a runtime '{request}' request, which a deterministic offline render cannot serve. Supply its data as inline content or drop the item from the curated catalog."
            );
        }
    }
    Ok(sanitized)
}

/// Makes one component mountable through `data-composition-src`.
///
/// The runtime renders a referenced file only when it holds `<template>` or
/// `<body>` content with an element carrying composition metadata. Part of the
/// registry's components are standalone documents that already qualify; the rest
/// are bare snippets meant to be pasted in by hand, which the author cannot do
/// because it is given item IDs and never their contents. Wrapping only what
/// needs it leaves one mounting rule covering the whole catalog, without
/// re-wrapping a document that already has its own root.
fn mountable_component(id: &str, content: &str, width: u32, height: u32) -> String {
    let lowercase = content.to_ascii_lowercase();
    if lowercase.contains("<template") || lowercase.contains("<body") {
        return content.to_string();
    }
    format!(
        "<template>\n<div id=\"{id}\" data-composition-id=\"{id}\" data-width=\"{width}\" data-height=\"{height}\" data-start=\"0\">\n{content}\n</div>\n</template>\n"
    )
}

/// Filesystem/process implementation for optional local video renderers.
#[derive(Clone)]
pub struct ManagedVideoRenderers {
    root: PathBuf,
    /// Manim is a Python package, so its interpreter is provisioned by the shared
    /// Python runtime rather than by a second venv story owned by this adapter.
    python: Arc<dyn PythonRuntime>,
}

impl ManagedVideoRenderers {
    /// Creates a manager rooted at an explicit directory.
    pub fn new(root: PathBuf, python: Arc<dyn PythonRuntime>) -> Self {
        Self { root, python }
    }

    /// Resolves the managed renderer root under `~/.sfumato/renderers`.
    pub fn default_root() -> Result<PathBuf> {
        Ok(dirs::home_dir()
            .context("Home directory is unavailable")?
            .join(".sfumato/renderers"))
    }

    /// Creates a manager under `~/.sfumato/renderers` with the default runtime.
    pub fn default_path() -> Result<Self> {
        Ok(Self::new(
            Self::default_root()?,
            Arc::new(UvPythonRuntime::default_path()?),
        ))
    }

    fn hyperframe_executable(&self) -> PathBuf {
        self.root.join("hyperframe/node_modules/.bin/hyperframes")
    }

    fn hyperframe_gsap_runtime(&self) -> PathBuf {
        self.root
            .join("hyperframe/node_modules/gsap/dist/gsap.min.js")
    }

    fn hyperframe_catalog(&self) -> PathBuf {
        self.root.join("hyperframe/catalog/manifest.json")
    }

    /// Project-shaped directory holding the installed catalog items.
    ///
    /// Lives under Sfumato's managed root, never inside a repository: the items
    /// are installed once per machine and copied into each render workspace.
    fn hyperframe_catalog_root(&self) -> PathBuf {
        self.root.join("hyperframe/catalog")
    }

    /// Where `hyperframes add` writes one item, mirroring the CLI's own layout.
    /// Reads one installed catalog item so the author can adapt its technique.
    fn read_catalog_item(&self, engine: VideoEngine, id: &str) -> Result<String> {
        if engine != VideoEngine::Hyperframe {
            bail!("Only the Hyperframe engine has a managed catalog");
        }
        let catalog = Self::parse_catalog()?;
        let item = catalog
            .find(id)
            .with_context(|| format!("Catalog item '{id}' is not curated"))?;
        let path = self.hyperframe_catalog_item(item);
        fs::read_to_string(&path)
            .with_context(|| format!("Could not read catalog item '{id}' at {}", path.display()))
    }

    fn hyperframe_catalog_item(&self, item: &VideoCatalogItem) -> PathBuf {
        let root = self.hyperframe_catalog_root();
        match item.kind {
            VideoCatalogKind::Block => root.join(format!("compositions/{}.html", item.id)),
            VideoCatalogKind::Component => {
                root.join(format!("compositions/components/{}.html", item.id))
            }
        }
    }

    fn parse_catalog() -> Result<VideoCatalog> {
        VideoCatalog::parse(HYPERFRAME_CATALOG)
            .map_err(|error| anyhow::anyhow!("{error}"))
            .context("Bundled Hyperframe catalog manifest is invalid")
    }

    /// Installs every curated catalog item into the managed catalog root.
    async fn install_hyperframe_catalog(&self, operation: &OperationContext) -> Result<()> {
        let catalog = Self::parse_catalog()?;
        let root = self.hyperframe_catalog_root();
        fs::create_dir_all(&root)?;
        let executable = self.hyperframe_executable();
        for item in catalog.items() {
            operation.checkpoint(OperationStage::Resolve)?;
            let mut command = Command::new(&executable);
            command
                .args(["add", &item.id, "--no-clipboard", "--json", "--dir"])
                .arg(&root)
                .env("HYPERFRAMES_NO_UPDATE_CHECK", "1")
                .env("HYPERFRAMES_NO_TELEMETRY", "1");
            checked(
                &mut command,
                operation,
                OperationStage::Resolve,
                &format!("Hyperframe catalog install for '{}'", item.id),
            )
            .await?;
            let installed = self.hyperframe_catalog_item(item);
            if !installed.is_file() {
                bail!(
                    "Hyperframe reported installing '{}' but {} is missing",
                    item.id,
                    installed.display()
                );
            }
        }
        fs::write(self.hyperframe_catalog(), HYPERFRAME_CATALOG)?;
        Ok(())
    }

    /// Stages the installed catalog items into one render workspace, offline.
    ///
    /// Mirrors how the managed GSAP runtime is vendored: the generated source
    /// references `compositions/<id>.html`, so the files have to sit beside it.
    /// Each item is rewritten on the way in rather than copied: the registry
    /// ships them with CDN and font URLs the render must not fetch, and ships
    /// components in a shape `data-composition-src` cannot mount.
    fn stage_hyperframe_catalog(&self, request: &VideoRenderRequest) -> Result<()> {
        let catalog = Self::parse_catalog()?;
        let source_root = &request.source_root;
        for item in catalog.items() {
            let installed = self.hyperframe_catalog_item(item);
            if !installed.is_file() {
                bail!(
                    "Managed catalog item '{}' is not installed. Run `sfumato renderer install hyperframe` again.",
                    item.id
                );
            }
            let staged = match item.kind {
                VideoCatalogKind::Block => {
                    source_root.join(format!("compositions/{}.html", item.id))
                }
                VideoCatalogKind::Component => {
                    source_root.join(format!("compositions/components/{}.html", item.id))
                }
            };
            let parent = staged.parent().expect("staged catalog item has a parent");
            fs::create_dir_all(parent)?;
            let content = fs::read_to_string(&installed)
                .with_context(|| format!("Could not read managed catalog item '{}'", item.id))?;
            let offline = offline_catalog_item(&item.id, &content)?;
            let mountable = match item.kind {
                VideoCatalogKind::Block => offline,
                VideoCatalogKind::Component => {
                    mountable_component(&item.id, &offline, request.width, request.height)
                }
            };
            fs::write(&staged, mountable)?;
        }
        Ok(())
    }

    /// Managed Paged.js CLI used to render documents.
    fn pagedjs_executable(&self) -> PathBuf {
        self.root.join("pagedjs/node_modules/.bin/pagedjs-cli")
    }

    /// The `manim` console script installed beside the managed interpreter.
    fn manim_executable(&self) -> Result<PathBuf> {
        let interpreter = self
            .python
            .interpreter_path("manim")
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(interpreter
            .parent()
            .context("Managed Python interpreter has no parent directory")?
            .join("manim"))
    }

    async fn renderer_status(
        &self,
        id: &str,
        operation: &OperationContext,
    ) -> Result<RendererStatus> {
        let package = renderer_package(id)?;
        let (executable, dependencies) = match id {
            "hyperframe" => (
                self.hyperframe_executable(),
                vec!["node", "ffmpeg", "ffprobe"],
            ),
            "manim" => (self.manim_executable()?, vec!["ffmpeg", "ffprobe"]),
            // The document renderer drives a browser through Node; the browser
            // itself is the one Sfumato already requires elsewhere.
            "pagedjs" => (self.pagedjs_executable(), vec!["node"]),
            _ => bail!("Unknown renderer '{id}'. Use hyperframe, manim, or pagedjs."),
        };
        let mut details = Vec::new();
        let installed = executable.is_file();
        let mut healthy = installed;
        if !installed {
            details.push(format!(
                "managed executable missing: {}",
                executable.display()
            ));
        }
        for dependency in dependencies {
            // Resolved so the probe agrees with what the renderers will actually
            // spawn; a tool reported present must be a tool that can be launched.
            let mut command = Command::new(crate::executables::resolve(dependency));
            command.arg(if dependency == "node" {
                "--version"
            } else {
                "-version"
            });
            match run_command(&mut command, operation, OperationStage::Resolve).await {
                Ok(output) if output.status.success() => {}
                Ok(_) | Err(_) => {
                    healthy = false;
                    details.push(format!("dependency missing: {dependency}"));
                }
            }
        }
        if id == "hyperframe" && installed && !self.hyperframe_gsap_runtime().is_file() {
            healthy = false;
            details.push(
                "managed runtime missing: gsap; run `sfumato renderer install hyperframe` again"
                    .into(),
            );
        }
        if id == "hyperframe" && installed {
            match fs::read_to_string(self.hyperframe_catalog()) {
                Ok(value) if value == HYPERFRAME_CATALOG => {
                    // The manifest matching is not enough on its own: the items
                    // it names are separate downloads that a partial install or
                    // a manual cleanup can remove.
                    let missing = Self::parse_catalog()?
                        .items()
                        .iter()
                        .filter(|item| !self.hyperframe_catalog_item(item).is_file())
                        .map(|item| item.id.clone())
                        .collect::<Vec<_>>();
                    if !missing.is_empty() {
                        healthy = false;
                        details.push(format!(
                            "managed catalog items not installed: {}; run `sfumato renderer install hyperframe` again",
                            missing.join(", ")
                        ));
                    }
                }
                _ => {
                    healthy = false;
                    details.push(
                        "managed Hyperframe catalog is missing or incompatible; run `sfumato renderer install hyperframe` again".into(),
                    );
                }
            }
        }
        if id == "hyperframe" && installed {
            let mut doctor = Command::new(&executable);
            doctor
                .args(["doctor", "--json"])
                .env("HYPERFRAMES_NO_UPDATE_CHECK", "1")
                .env("HYPERFRAMES_NO_TELEMETRY", "1");
            match run_command(&mut doctor, operation, OperationStage::Resolve).await {
                Ok(output) => {
                    match serde_json::from_slice::<HyperframesDoctorReport>(&output.stdout) {
                        Ok(report) => {
                            let (doctor_healthy, doctor_details) =
                                evaluate_hyperframes_doctor(report);
                            healthy &= doctor_healthy;
                            details.extend(doctor_details);
                        }
                        Err(_) => {
                            healthy = false;
                            details.push("Hyperframes doctor returned invalid JSON".into());
                        }
                    }
                }
                Err(error) => {
                    healthy = false;
                    details.push(format!("Hyperframes doctor failed: {error}"));
                }
            }
        }
        Ok(RendererStatus {
            id: id.into(),
            version: package.version,
            installed,
            healthy,
            details,
        })
    }
}

#[async_trait]
impl RendererManager for ManagedVideoRenderers {
    async fn list(&self, operation: &OperationContext) -> SfumatoResult<Vec<RendererStatus>> {
        managed_result(
            async {
                Ok(vec![
                    self.renderer_status("hyperframe", operation).await?,
                    self.renderer_status("manim", operation).await?,
                    self.renderer_status("pagedjs", operation).await?,
                ])
            }
            .await,
            OperationStage::Resolve,
        )
    }

    async fn install(
        &self,
        id: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<RendererStatus> {
        managed_result(
            async {
                fs::create_dir_all(&self.root)?;
                match id {
                    "hyperframe" => {
                        let package = renderer_package(id)?;
                        let prefix = self.root.join("hyperframe");
                        fs::create_dir_all(&prefix)?;
                        let mut command = Command::new(crate::executables::resolve("npm"));
                        command
                            .args(["install", "--no-audit", "--no-fund", "--prefix"])
                            .arg(&prefix)
                            .arg(format!("{}@{}", package.package, package.version));
                        for (runtime, version) in package.runtime_packages {
                            command.arg(format!("{runtime}@{version}"));
                        }
                        checked(
                            &mut command,
                            operation,
                            OperationStage::Resolve,
                            "Hyperframe installation",
                        )
                        .await?;
                        self.install_hyperframe_catalog(operation).await?;
                    }
                    "manim" => {
                        // Manim's pins live in the Python environment manifest, so
                        // installing it is the same operation the chart tool uses
                        // to provision its own interpreter.
                        self.python
                            .ensure("manim", &[], operation)
                            .await
                            .map_err(|error| anyhow::anyhow!("{error}"))?;
                    }
                    "pagedjs" => {
                        let package = renderer_package(id)?;
                        let prefix = self.root.join("pagedjs");
                        fs::create_dir_all(&prefix)?;
                        let mut command = Command::new(crate::executables::resolve("npm"));
                        command
                            .args(["install", "--no-audit", "--no-fund", "--prefix"])
                            .arg(&prefix)
                            .arg(format!("{}@{}", package.package, package.version));
                        checked(
                            &mut command,
                            operation,
                            OperationStage::Resolve,
                            "Paged.js CLI installation",
                        )
                        .await?;
                    }
                    _ => bail!("Unknown renderer '{id}'. Use hyperframe, manim, or pagedjs."),
                }
                self.renderer_status(id, operation).await
            }
            .await,
            OperationStage::Resolve,
        )
    }

    fn remove(&self, id: &str) -> SfumatoResult<RendererStatus> {
        let package =
            renderer_package(id).map_err(|error| SfumatoError::validation(error.to_string()))?;
        let path = match id {
            "hyperframe" => self.root.join("hyperframe"),
            "manim" => self.root.join("manim"),
            "pagedjs" => self.root.join("pagedjs"),
            _ => return Err(SfumatoError::validation(format!("Unknown renderer '{id}'"))),
        };
        if path.exists() {
            fs::remove_dir_all(&path)
                .map_err(|error| SfumatoError::render(ErrorClass::Permanent, error.to_string()))?;
        }
        Ok(RendererStatus {
            id: id.into(),
            version: package.version,
            installed: false,
            healthy: false,
            details: vec!["not installed".into()],
        })
    }

    async fn doctor(
        &self,
        id: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<RendererStatus>> {
        match id {
            Some(id) => self
                .renderer_status(id, operation)
                .await
                .map(|value| vec![value])
                .map_err(|error| {
                    SfumatoError::render(ErrorClass::Unavailable, format!("{error:#}"))
                }),
            None => self.list(operation).await,
        }
    }
}

#[async_trait]
impl VideoRenderer for ManagedVideoRenderers {
    async fn validate(
        &self,
        engine: VideoEngine,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> SfumatoResult<()> {
        let result = match engine {
            VideoEngine::Hyperframe => self.validate_hyperframe(request, operation).await,
            VideoEngine::Manim => self.validate_manim(request, operation).await,
            VideoEngine::Model => Err(anyhow::anyhow!(
                "Direct model videos do not use a local renderer"
            )),
        };
        managed_result(result, OperationStage::Render)
    }

    async fn snapshot(
        &self,
        engine: VideoEngine,
        request: &VideoRenderRequest,
        timestamps: &[f32],
        output_dir: &Path,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<PathBuf>> {
        let result = match engine {
            VideoEngine::Hyperframe => {
                self.snapshot_hyperframe(request, timestamps, output_dir, operation)
                    .await
            }
            VideoEngine::Manim | VideoEngine::Model => Ok(Vec::new()),
        };
        managed_result(result, OperationStage::InspectLayout)
    }

    async fn render(
        &self,
        engine: VideoEngine,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> SfumatoResult<()> {
        let result: Result<()> = match engine {
            VideoEngine::Hyperframe => self.render_hyperframe(request, operation).await,
            VideoEngine::Manim => self.render_manim(request, operation).await,
            VideoEngine::Model => Err(anyhow::anyhow!(
                "Direct model videos do not use a local renderer"
            )),
        };
        managed_result(result, OperationStage::Render)
    }

    async fn inspect(
        &self,
        video_path: &Path,
        operation: &OperationContext,
    ) -> SfumatoResult<VideoInspection> {
        managed_result(
            inspect_video(video_path, operation).await,
            OperationStage::InspectLayout,
        )
    }

    async fn measure_snapshots(
        &self,
        snapshots: &[(f32, PathBuf)],
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<VideoFrameMeasurement>> {
        let result = (|| {
            let mut measurements = Vec::with_capacity(snapshots.len());
            for (at_seconds, path) in snapshots {
                operation.checkpoint(OperationStage::InspectLayout)?;
                let decoded = image::open(path)
                    .with_context(|| format!("Could not read snapshot {}", path.display()))?
                    .to_rgba8();
                let (ink_ratio, distinct_colours) = measure_frame(&decoded);
                measurements.push(VideoFrameMeasurement {
                    at_seconds: *at_seconds,
                    ink_ratio,
                    distinct_colours,
                });
            }
            Ok(measurements)
        })();
        managed_result(result, OperationStage::InspectLayout)
    }

    fn catalog(&self, engine: VideoEngine) -> SfumatoResult<Option<VideoCatalog>> {
        match engine {
            // Only Hyperframe has a registry to install from; Manim composes
            // scenes in Python and a direct model receives a prompt.
            VideoEngine::Hyperframe => {
                managed_result(Self::parse_catalog().map(Some), OperationStage::Resolve)
            }
            VideoEngine::Manim | VideoEngine::Model => Ok(None),
        }
    }

    fn catalog_item_source(&self, engine: VideoEngine, id: &str) -> SfumatoResult<String> {
        managed_result(self.read_catalog_item(engine, id), OperationStage::Resolve)
    }
}

impl ManagedVideoRenderers {
    /// Compiles every authored scene before the film is rendered.
    ///
    /// This is what makes a malformed module repairable. The repair loop is driven
    /// by validation, not by rendering, so without a check here a scene that does
    /// not parse would fail the whole film at the last step with no chance to be
    /// re-authored — and a syntax error is the most common way generated Python
    /// comes back wrong.
    async fn validate_manim(
        &self,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> Result<()> {
        let manifest = read_manim_manifest(&request.source_root)?;
        let python = self
            .python
            .interpreter_path("manim")
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        if !python.is_file() {
            bail!("Manim is not installed. Run `sfumato renderer install manim`.");
        }
        let mut modules = manifest
            .scenes
            .iter()
            .map(|scene| scene.module.clone())
            .collect::<Vec<_>>();
        // The caption overlay is generated rather than authored, but it is Python
        // that has to run, so a defect in it should surface here too.
        if let Some(captions) = &manifest.captions {
            modules.push(captions.module.clone());
        }
        for module in &modules {
            let path = request.source_root.join(module);
            if !path.is_file() {
                bail!("Manim source is missing {module}");
            }
            let mut compile = Command::new(&python);
            compile
                .args(["-m", "py_compile"])
                .arg(module)
                // Byte-code caches are a side effect of checking, not part of the
                // film. Pointed at a scratch directory they never reach the
                // committed revision.
                .env(
                    "PYTHONPYCACHEPREFIX",
                    request.source_root.join(".dry-run/cache"),
                )
                .current_dir(&request.source_root);
            checked(
                &mut compile,
                operation,
                OperationStage::Render,
                &format!("Manim syntax check for {module}"),
            )
            .await?;
        }

        // Compiling proves a module parses, not that it runs. A scene that calls
        // an animation with the wrong kind of mobject raises only once Manim
        // builds it, and that used to surface at the final render — after every
        // scene had been authored and narrated — where nothing could repair it.
        // A dry run executes `construct` at the lowest quality without writing
        // any output, so a runtime fault costs about a second per scene and
        // arrives while the scene can still be re-authored.
        let executable = self.manim_executable()?;
        for scene in &manifest.scenes {
            operation.checkpoint(OperationStage::Render)?;
            let mut probe = Command::new(&executable);
            probe
                .arg("render")
                .arg("--dry_run")
                .args(["-q", "l"])
                .arg("--media_dir")
                .arg(request.source_root.join(".dry-run"))
                .arg(&scene.module)
                .arg(&scene.class_name)
                .current_dir(&request.source_root);
            checked(
                &mut probe,
                operation,
                OperationStage::Render,
                &format!("Manim dry run for {}", scene.module),
            )
            .await?;
        }
        fs::remove_dir_all(request.source_root.join(".dry-run")).ok();
        Ok(())
    }

    async fn validate_hyperframe(
        &self,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> Result<()> {
        let executable = self.hyperframe_executable();
        if !executable.is_file() {
            bail!("Hyperframe is not installed. Run `sfumato renderer install hyperframe`.");
        }
        let gsap = self.hyperframe_gsap_runtime();
        if !gsap.is_file() {
            bail!(
                "Hyperframe's managed GSAP runtime is missing. Run `sfumato renderer install hyperframe` again."
            );
        }
        let vendor = request.source_root.join("vendor");
        fs::create_dir_all(&vendor)?;
        fs::copy(gsap, vendor.join("gsap.min.js"))?;
        self.stage_hyperframe_catalog(request)?;
        let mut command = Command::new(&executable);
        command
            .arg("check")
            .env("HYPERFRAMES_NO_UPDATE_CHECK", "1")
            .env("HYPERFRAMES_NO_TELEMETRY", "1")
            .current_dir(&request.source_root);
        let output = run_command(&mut command, operation, OperationStage::Render)
            .await
            .context("Could not run Hyperframe check")?;
        if !output.status.success() {
            bail!(
                "Hyperframe check failed:\n{}",
                check_failure_message(
                    &String::from_utf8_lossy(&output.stdout),
                    &String::from_utf8_lossy(&output.stderr),
                )
            );
        }
        Ok(())
    }

    async fn snapshot_hyperframe(
        &self,
        request: &VideoRenderRequest,
        timestamps: &[f32],
        output_dir: &Path,
        operation: &OperationContext,
    ) -> Result<Vec<PathBuf>> {
        if timestamps.is_empty() {
            return Ok(Vec::new());
        }
        let executable = self.hyperframe_executable();
        let before = collect_pngs(&request.source_root);
        let at = timestamps
            .iter()
            .map(|value| format!("{value:.3}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut command = Command::new(&executable);
        command
            .args(["snapshot", "--at", &at])
            .env("HYPERFRAMES_NO_UPDATE_CHECK", "1")
            .env("HYPERFRAMES_NO_TELEMETRY", "1")
            .current_dir(&request.source_root);
        checked(
            &mut command,
            operation,
            OperationStage::InspectLayout,
            "Hyperframe snapshot",
        )
        .await?;
        fs::create_dir_all(output_dir)?;
        let mut copied = Vec::new();
        for path in collect_pngs(&request.source_root) {
            if before.contains(&path) {
                continue;
            }
            let destination = output_dir.join(path.file_name().unwrap_or_default());
            fs::copy(&path, &destination)?;
            copied.push(destination);
        }
        Ok(copied)
    }

    async fn render_hyperframe(
        &self,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> Result<()> {
        self.validate_hyperframe(request, operation).await?;
        let executable = self.hyperframe_executable();
        let mut render = Command::new(&executable);
        render
            .arg("render")
            .arg("--output")
            .arg(&request.output_path)
            .arg("--fps")
            .arg(request.fps.to_string())
            .arg("--quality")
            .arg(&request.quality)
            .env("HYPERFRAMES_NO_UPDATE_CHECK", "1")
            .env("HYPERFRAMES_NO_TELEMETRY", "1")
            .current_dir(&request.source_root);
        checked(
            &mut render,
            operation,
            OperationStage::Render,
            "Hyperframe render",
        )
        .await
    }

    /// Renders every planned scene and assembles them into one film.
    ///
    /// Manim renders a scene at a time and has no concept of a film, so the
    /// timeline lives in the generated manifest and is replayed here: render each
    /// module, concatenate in plan order, mix the narration in at its recorded
    /// offsets, and burn the caption track over the result. Doing the audio and
    /// captions in post rather than inside `construct` keeps a scene's Python
    /// about the picture, and lets a caption span a scene boundary at all.
    async fn render_manim(
        &self,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> Result<()> {
        let executable = self.manim_executable()?;
        if !executable.is_file() {
            bail!("Manim is not installed. Run `sfumato renderer install manim`.");
        }
        // Re-checked here so a render is never attempted on source that would not
        // have passed validation, however this renderer was reached.
        self.validate_manim(request, operation).await?;
        let manifest = read_manim_manifest(&request.source_root)?;
        let media = request.source_root.join(".media");

        let mut clips = Vec::with_capacity(manifest.scenes.len());
        for scene in &manifest.scenes {
            operation.checkpoint(OperationStage::Render)?;
            let module = request.source_root.join(&scene.module);
            let output_name = format!("{}.mp4", scene.class_name);
            let mut render = Command::new(&executable);
            render
                .arg("render")
                .arg("--format")
                .arg("mp4")
                .arg("--fps")
                .arg(request.fps.to_string())
                .arg("--resolution")
                .arg(format!("{},{}", request.width, request.height))
                .arg("--media_dir")
                .arg(&media)
                .arg("--output_file")
                .arg(&output_name)
                .arg(&module)
                .arg(&scene.class_name)
                .current_dir(&request.source_root);
            checked(
                &mut render,
                operation,
                OperationStage::Render,
                &format!("Manim render for scene '{}'", scene.id),
            )
            .await?;
            clips.push(find_rendered_clip(&media, &output_name).with_context(|| {
                format!(
                    "Manim rendered scene '{}' without producing {output_name}",
                    scene.id
                )
            })?);
        }

        let staging = request.source_root.join(".assembly");
        // Rebuilt each run so a stale clip from an earlier attempt cannot be
        // concatenated into this one.
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        fs::create_dir_all(&staging)?;
        let silent = staging.join("silent.mp4");
        concat_clips(&clips, &staging, &silent, operation).await?;

        // The caption overlay is a Manim render too, with a transparent
        // background, so compositing it needs only FFmpeg's core `overlay`. The
        // alternative — burning a subtitle file — needs an FFmpeg built with
        // libass, which many installations do not have.
        let captions = match &manifest.captions {
            Some(captions) => {
                let module = request.source_root.join(&captions.module);
                if !module.is_file() {
                    bail!("Manim source is missing {}", captions.module);
                }
                let output_name = format!("{}.mov", captions.class_name);
                let mut render = Command::new(&executable);
                render
                    .arg("render")
                    .arg("--format")
                    .arg("mov")
                    .arg("--transparent")
                    .arg("--fps")
                    .arg(request.fps.to_string())
                    .arg("--resolution")
                    .arg(format!("{},{}", request.width, request.height))
                    .arg("--media_dir")
                    .arg(&media)
                    .arg("--output_file")
                    .arg(&output_name)
                    .arg(&module)
                    .arg(&captions.class_name)
                    .current_dir(&request.source_root);
                checked(
                    &mut render,
                    operation,
                    OperationStage::Render,
                    "Manim caption overlay render",
                )
                .await?;
                Some(find_rendered_clip(&media, &output_name).with_context(|| {
                    format!("Manim rendered captions without producing {output_name}")
                })?)
            }
            None => None,
        };

        compose_film(ComposeFilmRequest {
            silent: &silent,
            captions: captions.as_deref(),
            manifest: &manifest,
            source_root: &request.source_root,
            staging: &staging,
            output_path: &request.output_path,
            operation,
        })
        .await?;
        // Both working directories sit inside the source root, which is what the
        // transaction commits. Left behind, every revision would carry a second
        // copy of the film in per-scene pieces plus an uncompressed overlay.
        fs::remove_dir_all(&staging).ok();
        fs::remove_dir_all(&media).ok();
        Ok(())
    }
}

/// The generated timeline a Manim source carries beside its scenes.
#[derive(Clone, Debug, Deserialize)]
struct ManimManifest {
    scenes: Vec<ManimSceneEntry>,
    #[serde(default)]
    audio: Vec<ManimAudioEntry>,
    #[serde(default)]
    captions: Option<ManimCaptionEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManimCaptionEntry {
    module: String,
    class_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ManimSceneEntry {
    id: String,
    module: String,
    class_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ManimAudioEntry {
    reference: String,
    start_seconds: f32,
}

fn read_manim_manifest(source_root: &Path) -> Result<ManimManifest> {
    let path = source_root.join("manifest.json");
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("Could not read the Manim manifest at {}", path.display()))?;
    let manifest: ManimManifest =
        serde_json::from_str(&contents).context("Manim manifest is invalid")?;
    if manifest.scenes.is_empty() {
        bail!("Manim manifest declares no scenes");
    }
    Ok(manifest)
}

/// Finds the MP4 Manim wrote, wherever its media layout put it.
///
/// Manim nests its output under a directory named for the module and the
/// resolution, so the file is located by name rather than by a path this adapter
/// would have to keep in sync with Manim's own conventions.
fn find_rendered_clip(media: &Path, file_name: &str) -> Option<PathBuf> {
    WalkDir::new(media)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(file_name))
}

/// Joins the rendered scenes into one continuous silent film.
///
/// The concat demuxer is used rather than a filter graph because every clip came
/// out of the same renderer at the same resolution and frame rate, so the streams
/// can be copied instead of re-encoded.
async fn concat_clips(
    clips: &[PathBuf],
    staging: &Path,
    output: &Path,
    operation: &OperationContext,
) -> Result<()> {
    let list = staging.join("clips.txt");
    let mut manifest = String::new();
    for clip in clips {
        // The demuxer treats a quote as a delimiter, so a path carrying one has to
        // be escaped rather than trusted.
        manifest.push_str(&format!(
            "file '{}'\n",
            clip.display().to_string().replace('\'', r"'\''")
        ));
    }
    fs::write(&list, manifest).with_context(|| format!("Could not write {}", list.display()))?;
    let mut concat = Command::new(crate::executables::resolve("ffmpeg"));
    concat
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .args(["-c", "copy"])
        .arg(output);
    checked(
        &mut concat,
        operation,
        OperationStage::Render,
        "Manim scene concatenation",
    )
    .await
}

struct ComposeFilmRequest<'a> {
    silent: &'a Path,
    captions: Option<&'a Path>,
    manifest: &'a ManimManifest,
    source_root: &'a Path,
    staging: &'a Path,
    output_path: &'a Path,
    operation: &'a OperationContext,
}

/// Mixes the narration under the film and burns the captions over it.
///
/// One pass rather than two: each narration clip is delayed to its recorded start
/// and the results are mixed, while the subtitle filter draws the caption track
/// onto the video. Splitting this into separate passes would re-encode the video
/// twice for no benefit.
async fn compose_film(request: ComposeFilmRequest<'_>) -> Result<()> {
    let ComposeFilmRequest {
        silent,
        captions,
        manifest,
        source_root,
        staging,
        output_path,
        operation,
    } = request;

    if manifest.audio.is_empty() && captions.is_none() {
        fs::copy(silent, output_path).with_context(|| {
            format!(
                "Could not write the rendered film to {}",
                output_path.display()
            )
        })?;
        return Ok(());
    }

    let mut command = Command::new(crate::executables::resolve("ffmpeg"));
    command.args(["-y", "-i"]).arg(silent);
    if let Some(captions) = captions {
        command.arg("-i").arg(captions);
    }
    for clip in &manifest.audio {
        let path = source_root.join(&clip.reference);
        if !path.is_file() {
            bail!("Narration clip {} is missing", clip.reference);
        }
        command.arg("-i").arg(&path);
    }

    // Audio inputs sit after the video and the optional overlay.
    let first_audio_input = if captions.is_some() { 2 } else { 1 };
    let mut filters = Vec::new();
    if captions.is_some() {
        // `eof_action=pass` so a caption track that ends early leaves the rest of
        // the film untouched rather than ending the output with it.
        filters.push("[0:v][1:v]overlay=eof_action=pass[v]".to_string());
    }
    if !manifest.audio.is_empty() {
        for (index, clip) in manifest.audio.iter().enumerate() {
            let delay = (clip.start_seconds.max(0.0) * 1_000.0).round() as u64;
            // Both channels are delayed: `adelay` leaves any channel it is not
            // given a value for at zero, which would play half the narration early.
            filters.push(format!(
                "[{}:a]adelay={delay}|{delay}[a{index}]",
                first_audio_input + index
            ));
        }
        let inputs = (0..manifest.audio.len())
            .map(|index| format!("[a{index}]"))
            .collect::<String>();
        // Padded with silence, then cut to the video by `-shortest`. Narration
        // almost always ends before the last scene does, because a final beat
        // holds its picture after the voice stops; without the pad, `-shortest`
        // would end the film at the last spoken word and drop those frames.
        filters.push(format!(
            "{inputs}amix=inputs={}:normalize=0,apad[a]",
            manifest.audio.len()
        ));
    }
    command.arg("-filter_complex").arg(filters.join(";"));
    command
        .arg("-map")
        .arg(if captions.is_some() { "[v]" } else { "0:v" });
    if manifest.audio.is_empty() {
        command.args(["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
    } else {
        command
            .args(["-map", "[a]"])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
            // The film is as long as its pictures; a narration clip that overruns
            // the last scene must not extend it past the frames that exist.
            .arg("-shortest");
    }
    let composed = staging.join("composed.mp4");
    command.arg(&composed);
    checked(
        &mut command,
        operation,
        OperationStage::Render,
        "Manim narration and caption mux",
    )
    .await?;
    fs::copy(&composed, output_path).with_context(|| {
        format!(
            "Could not write the rendered film to {}",
            output_path.display()
        )
    })?;
    Ok(())
}

/// Colour buckets per channel when counting distinct colours.
///
/// Coarse on purpose: a gradient is one visual element, not thousands of colours,
/// and quantising keeps antialiasing from reading as content.
const COLOUR_BUCKETS: u32 = 6;

/// How far a pixel must sit from the dominant colour to count as ink.
///
/// Antialiased edges and subtle theme gradients differ from the background by a
/// hair; only a real mark clears this.
const INK_DISTANCE: u32 = 24;

/// Measures one captured frame.
///
/// The dominant colour stands in for the background, so the measurement works for
/// any theme without being told what the background is.
pub(crate) fn measure_frame(image: &image::RgbaImage) -> (f32, u32) {
    let bucket = |value: u8| u8::try_from(u32::from(value) * COLOUR_BUCKETS / 256).unwrap_or(0);
    let mut histogram: BTreeMap<(u8, u8, u8), u32> = BTreeMap::new();
    for pixel in image.pixels() {
        *histogram
            .entry((bucket(pixel[0]), bucket(pixel[1]), bucket(pixel[2])))
            .or_default() += 1;
    }
    let distinct = u32::try_from(histogram.len()).unwrap_or(u32::MAX);
    let Some((dominant, _)) = histogram.iter().max_by_key(|(_, count)| **count) else {
        return (0.0, 0);
    };
    let dominant = *dominant;

    // The reference is the mean of the pixels in the dominant bucket, not the
    // bucket's index scaled back up: a bucket spans 42 levels, so reconstructing
    // from the index lands far enough from the real background that every pixel
    // reads as ink.
    let mut sums = (0_u64, 0_u64, 0_u64);
    let mut count = 0_u64;
    for pixel in image.pixels() {
        if (bucket(pixel[0]), bucket(pixel[1]), bucket(pixel[2])) == dominant {
            sums.0 += u64::from(pixel[0]);
            sums.1 += u64::from(pixel[1]);
            sums.2 += u64::from(pixel[2]);
            count += 1;
        }
    }
    if count == 0 {
        return (0.0, distinct);
    }
    let reference = (
        u32::try_from(sums.0 / count).unwrap_or(0),
        u32::try_from(sums.1 / count).unwrap_or(0),
        u32::try_from(sums.2 / count).unwrap_or(0),
    );

    let mut ink = 0_u64;
    for pixel in image.pixels() {
        let distance = u32::from(pixel[0]).abs_diff(reference.0)
            + u32::from(pixel[1]).abs_diff(reference.1)
            + u32::from(pixel[2]).abs_diff(reference.2);
        if distance > INK_DISTANCE {
            ink += 1;
        }
    }
    let total = u64::from(image.width()) * u64::from(image.height());
    let ratio = if total == 0 {
        0.0
    } else {
        ink as f32 / total as f32
    };
    (ratio, distinct)
}

/// Keeps only the lines a caller can act on when the renderer's check fails.
///
/// The check prints a full lint report: hundreds of lines of warnings and notes
/// about every staged catalog item, with the actual errors last. Messages are
/// capped in length before they reach a user, so dumping the whole report
/// reliably truncated away the errors and left advice about files the film does
/// not even use.
pub(crate) fn check_failure_message(stdout: &str, stderr: &str) -> String {
    const MAX_LINES: usize = 24;
    let lines = stdout.lines().chain(stderr.lines()).collect::<Vec<_>>();
    let mut kept = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // The renderer marks errors with a cross and closes each phase with a
        // count; everything else is advice about how a file could be nicer.
        let is_error = trimmed.starts_with('\u{2717}') || trimmed.starts_with("✗");
        let is_summary = trimmed.contains("error(s)") && !trimmed.starts_with("0 error(s)");
        if is_error || is_summary {
            kept.push(trimmed.to_string());
        }
        // The offending path sits on its own line under the marker. Keeping it is
        // what lets a repair target the one scene at fault instead of re-authoring
        // the whole film, so it is worth a line of the budget.
        if is_error
            && let Some(location) = lines.get(index + 1)
            && let location = location.trim()
            && location.contains(".html")
        {
            kept.push(location.to_string());
        }
        if kept.len() >= MAX_LINES {
            kept.push("... further errors omitted".to_string());
            break;
        }
    }
    if kept.is_empty() {
        // No cross-marked line: the check failed for a reason it did not itemise,
        // so the tail is more useful than the head that a cap would keep.
        let tail = stdout
            .lines()
            .chain(stderr.lines())
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        let start = tail.len().saturating_sub(MAX_LINES);
        return tail[start..].join("\n");
    }
    kept.join("\n")
}

async fn inspect_video(path: &Path, operation: &OperationContext) -> Result<VideoInspection> {
    if !path.is_file() {
        bail!("Rendered video does not exist at {}", path.display());
    }
    let mut command = Command::new(crate::executables::resolve("ffprobe"));
    command
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path);
    let output = checked_output(
        &mut command,
        operation,
        OperationStage::InspectLayout,
        "ffprobe",
    )
    .await?;
    let probe: Probe =
        serde_json::from_slice(&output.stdout).context("ffprobe returned invalid JSON")?;
    let video = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video")
        .context("MP4 does not contain a video stream")?;
    let duration_seconds = probe
        .format
        .duration
        .as_deref()
        .unwrap_or("0")
        .parse::<f64>()?;
    if !probe.format.format_name.contains("mp4") {
        bail!("Rendered video is not an MP4-compatible container");
    }
    if duration_seconds <= 0.0 || video.width.unwrap_or(0) == 0 || video.height.unwrap_or(0) == 0 {
        bail!("Rendered MP4 has invalid duration or dimensions");
    }
    Ok(VideoInspection {
        duration_seconds,
        width: video.width.unwrap_or(0),
        height: video.height.unwrap_or(0),
        has_audio: probe
            .streams
            .iter()
            .any(|stream| stream.codec_type == "audio"),
        video_codec: video.codec_name.clone().unwrap_or_default(),
    })
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}
#[derive(Deserialize)]
struct ProbeStream {
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}
#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    format_name: String,
}

async fn checked(
    command: &mut Command,
    operation: &OperationContext,
    stage: OperationStage,
    label: &str,
) -> Result<()> {
    checked_output(command, operation, stage, label)
        .await
        .map(|_| ())
}

async fn checked_output(
    command: &mut Command,
    operation: &OperationContext,
    stage: OperationStage,
    label: &str,
) -> Result<std::process::Output> {
    let output = run_command(command, operation, stage)
        .await
        .with_context(|| format!("Could not run {label}"))?;
    if !output.status.success() {
        // Filtered the same way a failed check is: these renderers print pages of
        // lint advice before the errors, and the raw head is what the message cap
        // keeps, so the reason was routinely invisible.
        bail!(
            "{label} failed with {}:\n{}",
            output.status,
            check_failure_message(
                &String::from_utf8_lossy(&output.stdout),
                &String::from_utf8_lossy(&output.stderr),
            )
        );
    }
    Ok(output)
}

fn managed_result<T>(result: Result<T>, stage: OperationStage) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            return error.clone();
        }
        let message = format!("{error:#}");
        let class = if message.contains("not installed") || message.contains("dependency missing") {
            ErrorClass::Unavailable
        } else {
            ErrorClass::Permanent
        };
        SfumatoError::render(class, message).at_stage(stage)
    })
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests validate the managed renderer manifest.
#[path = "../tests/unit/videos.rs"]
mod tests;
