//! Managed Hyperframe/Manim installation, rendering, and MP4 inspection.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    renderers::{
        RendererManager, RendererStatus, VideoCatalog, VideoCatalogItem, VideoCatalogKind,
        VideoEngine, VideoInspection, VideoRenderRequest, VideoRenderer,
    },
};
use tokio::process::Command;
use walkdir::WalkDir;

use crate::runtime::run_command;

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
        if REMOTE_FONT_HOSTS.iter().any(|host| statement.contains(host)) {
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
#[derive(Clone, Debug)]
pub struct ManagedVideoRenderers {
    root: PathBuf,
}

impl ManagedVideoRenderers {
    /// Creates a manager rooted at an explicit directory.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Creates a manager under `~/.sfumato/renderers`.
    pub fn default_path() -> Result<Self> {
        Ok(Self::new(
            dirs::home_dir()
                .context("Home directory is unavailable")?
                .join(".sfumato/renderers"),
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
            let content = fs::read_to_string(&installed).with_context(|| {
                format!("Could not read managed catalog item '{}'", item.id)
            })?;
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

    fn manim_executable(&self) -> PathBuf {
        self.root.join("manim/.venv/bin/manim")
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
            "manim" => (self.manim_executable(), vec!["ffmpeg", "ffprobe"]),
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
            let mut command = Command::new(dependency);
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
                        let mut command = Command::new("npm");
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
                        let package = renderer_package(id)?;
                        let root = self.root.join("manim");
                        fs::create_dir_all(&root)?;
                        let environment = root.join(".venv");
                        let mut venv = Command::new("uv");
                        venv.args(["venv", "--python", "3.12"]).arg(&environment);
                        checked(
                            &mut venv,
                            operation,
                            OperationStage::Resolve,
                            "Manim environment creation",
                        )
                        .await?;
                        let python = environment.join("bin/python");
                        let mut install = Command::new("uv");
                        install
                            .args(["pip", "install", "--python"])
                            .arg(&python)
                            .arg(format!("{}=={}", package.package, package.version));
                        checked(
                            &mut install,
                            operation,
                            OperationStage::Resolve,
                            "Manim installation",
                        )
                        .await?;
                    }
                    "pagedjs" => {
                        let package = renderer_package(id)?;
                        let prefix = self.root.join("pagedjs");
                        fs::create_dir_all(&prefix)?;
                        let mut command = Command::new("npm");
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
            VideoEngine::Manim => Ok(()),
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
}

impl ManagedVideoRenderers {
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
        checked(
            &mut command,
            operation,
            OperationStage::Render,
            "Hyperframe check",
        )
        .await
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

    async fn render_manim(
        &self,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> Result<()> {
        let executable = self.manim_executable();
        if !executable.is_file() {
            bail!("Manim is not installed. Run `sfumato renderer install manim`.");
        }
        let scene = request.source_root.join("scene.py");
        let python = self.root.join("manim/.venv/bin/python");
        let mut compile = Command::new(&python);
        compile
            .args(["-m", "py_compile"])
            .arg(&scene)
            .current_dir(&request.source_root);
        checked(
            &mut compile,
            operation,
            OperationStage::Render,
            "Manim Python syntax check",
        )
        .await?;
        let media = request.source_root.join(".media");
        let mut command = Command::new(&executable);
        command
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
            .arg("sfumato-video.mp4")
            .arg(&scene)
            .arg("SfumatoScene")
            .current_dir(&request.source_root);
        checked(
            &mut command,
            operation,
            OperationStage::Render,
            "Manim render",
        )
        .await?;
        let rendered = WalkDir::new(&media)
            .into_iter()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .find(|path| {
                path.file_name().and_then(|name| name.to_str()) == Some("sfumato-video.mp4")
            })
            .context("Manim completed without producing sfumato-video.mp4")?;
        fs::copy(&rendered, &request.output_path)?;
        Ok(())
    }
}

async fn inspect_video(path: &Path, operation: &OperationContext) -> Result<VideoInspection> {
    if !path.is_file() {
        bail!("Rendered video does not exist at {}", path.display());
    }
    let mut command = Command::new("ffprobe");
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
        bail!(
            "{label} failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
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
