//! Remote plugin discovery and versioned user-local plugin storage.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    page_plugins::{
        DownloadedPagePluginPackage, PagePluginCatalog, PagePluginCdnAsset,
        PagePluginInstallRecipe, PagePluginPackage, PagePluginRegistry, PagePluginRelease,
        PagePluginSource, PagePluginSummary, SupportedPagePlugin, validate_plugin_id,
    },
};
use sha2::{Digest, Sha256};

use crate::runtime::await_operation;

const BUILTIN_REGISTRY: &[u8] = include_bytes!("../assets/page-plugin-registry.json");
const DEFAULT_REGISTRY_URL: &str = "https://raw.githubusercontent.com/getsfumato/cli/master/crates/sfumato-adapters/assets/page-plugin-registry.json";

/// Metadata-only registry with direct public-CDN installation.
#[derive(Clone, Debug)]
pub struct CdnPagePluginSource {
    client: reqwest::Client,
    registry_url: String,
    cache_path: PathBuf,
}

impl CdnPagePluginSource {
    /// Creates the production CDN source.
    pub fn new() -> Result<Self> {
        let root = plugin_store_root()?;
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            registry_url: std::env::var("SFUMATO_PLUGIN_REGISTRY_URL")
                .unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string()),
            cache_path: root.join("registry.json"),
        })
    }

    async fn download_asset(
        &self,
        plugin: &str,
        asset: &PagePluginCdnAsset,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<u8>> {
        let response = await_operation(
            operation,
            OperationStage::Resolve,
            self.client.get(&asset.url).send(),
        )
        .await
        .map_err(plugin_transport_error)?;
        if !response.status().is_success() {
            return Err(SfumatoError::provider(
                ErrorClass::Unavailable,
                format!(
                    "CDN asset '{}' for plugin '{plugin}' returned HTTP {}",
                    asset.name,
                    response.status()
                ),
            ));
        }
        let bytes = await_operation(operation, OperationStage::Resolve, response.bytes())
            .await
            .map_err(plugin_transport_error)?;
        verify_hash(
            &bytes,
            &asset.sha256,
            &format!("CDN asset '{}'", asset.name),
        )?;
        Ok(bytes.to_vec())
    }
}

#[async_trait]
impl PagePluginSource for CdnPagePluginSource {
    async fn registry(&self, operation: &OperationContext) -> SfumatoResult<PagePluginRegistry> {
        let response = await_operation(
            operation,
            OperationStage::Resolve,
            self.client
                .get(&self.registry_url)
                .timeout(std::time::Duration::from_secs(3))
                .send(),
        )
        .await;
        let remote = match response {
            Ok(response) if response.status().is_success() => {
                await_operation(operation, OperationStage::Resolve, response.bytes())
                    .await
                    .map_err(plugin_transport_error)
                    .and_then(|bytes| parse_registry(&bytes).map(|registry| (bytes, registry)))
            }
            Ok(response) => Err(SfumatoError::provider(
                ErrorClass::Unavailable,
                format!("Plugin metadata returned HTTP {}", response.status()),
            )),
            Err(error) => Err(plugin_transport_error(error)),
        };
        match remote {
            Ok((bytes, registry)) => {
                write_atomic(&self.cache_path, &bytes).map_err(plugin_store_error)?;
                Ok(registry)
            }
            Err(_) => self.cached_or_builtin_registry(),
        }
    }

    async fn download(
        &self,
        plugin: &SupportedPagePlugin,
        release: &PagePluginRelease,
        operation: &OperationContext,
    ) -> SfumatoResult<DownloadedPagePluginPackage> {
        let (runtime_javascript, source_guidance) = match &release.install {
            PagePluginInstallRecipe::ClassicGlobal {
                runtime,
                source_global,
            } => {
                validate_global_expression(&release.api_global)?;
                validate_global_expression(source_global)?;
                let runtime = self.download_asset(&plugin.id, runtime, operation).await?;
                let runtime = String::from_utf8(runtime).map_err(|error| {
                    SfumatoError::validation(format!(
                        "Plugin '{}' runtime is not UTF-8: {error}",
                        plugin.id
                    ))
                })?;
                let assignment = if release.api_global == *source_global {
                    String::new()
                } else {
                    format!("\n{} = {};\n", release.api_global, source_global)
                };
                (format!("{runtime}{assignment}"), String::new())
            }
            PagePluginInstallRecipe::EsmNamespace { entry, imports } => {
                validate_global_expression(&release.api_global)?;
                let mut entry =
                    String::from_utf8(self.download_asset(&plugin.id, entry, operation).await?)
                        .map_err(|error| {
                            SfumatoError::validation(format!(
                                "Plugin '{}' module is not UTF-8: {error}",
                                plugin.id
                            ))
                        })?;
                for import in imports {
                    let module = self
                        .download_asset(&plugin.id, &import.asset, operation)
                        .await?;
                    let data_url = format!("data:text/javascript;base64,{}", BASE64.encode(module));
                    let double = format!("\"{}\"", import.specifier);
                    let single = format!("'{}'", import.specifier);
                    if !entry.contains(&double) && !entry.contains(&single) {
                        return Err(SfumatoError::validation(format!(
                            "Plugin '{}' entry module does not import '{}'",
                            plugin.id, import.specifier
                        )));
                    }
                    entry = entry.replace(&double, &format!("\"{data_url}\""));
                    entry = entry.replace(&single, &format!("'{data_url}'"));
                }
                let entry_url = format!(
                    "data:text/javascript;base64,{}",
                    BASE64.encode(entry.as_bytes())
                );
                (
                    format!("{} = import(\"{entry_url}\");", release.api_global),
                    String::new(),
                )
            }
            PagePluginInstallRecipe::TailwindSource { runtime, sources } => {
                validate_global_expression(&release.api_global)?;
                let runtime =
                    String::from_utf8(self.download_asset(&plugin.id, runtime, operation).await?)
                        .map_err(|error| {
                        SfumatoError::validation(format!(
                            "Plugin '{}' Tailwind runtime is not UTF-8: {error}",
                            plugin.id
                        ))
                    })?;
                let mut definitions = Vec::new();
                for source in sources {
                    let content = String::from_utf8(
                        self.download_asset(&plugin.id, source, operation).await?,
                    )
                    .map_err(|error| {
                        SfumatoError::validation(format!(
                            "Plugin '{}' source '{}' is not UTF-8: {error}",
                            plugin.id, source.name
                        ))
                    })?;
                    definitions.push((source.name.clone(), content));
                }
                let source_names = definitions
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>();
                let names_json = serde_json::to_string(&source_names).map_err(|error| {
                    SfumatoError::internal(format!("Could not serialize plugin sources: {error}"))
                })?;
                let bootstrap = format!(
                    r#"(()=>{{const style=document.createElement('style');style.type='text/tailwindcss';style.dataset.sfumatoPlugin='{}';style.textContent=`{}`;document.head.appendChild(style);}})();
{}
{} = {{kind:'source', components:{}}};"#,
                    plugin.id,
                    tailwind_theme_source(),
                    runtime,
                    release.api_global,
                    names_json,
                );
                let guidance = definitions
                    .into_iter()
                    .map(|(name, content)| format!("\n\n### {name}\n```json\n{content}\n```"))
                    .collect::<String>();
                (bootstrap, guidance)
            }
        };
        let mut license_text = String::new();
        for license in &release.licenses {
            let content =
                String::from_utf8(self.download_asset(&plugin.id, license, operation).await?)
                    .map_err(|error| {
                        SfumatoError::validation(format!(
                            "Plugin '{}' license '{}' is not UTF-8: {error}",
                            plugin.id, license.name
                        ))
                    })?;
            license_text.push_str(&format!("===== {} =====\n{}\n\n", license.name, content));
        }
        let runtime_hash = format!("{:x}", Sha256::digest(runtime_javascript.as_bytes()));
        let package = DownloadedPagePluginPackage {
            schema_version: sfumato_core::page_plugins::PAGE_PLUGIN_PACKAGE_SCHEMA_VERSION,
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            version: release.version.clone(),
            api_global: release.api_global.clone(),
            category: plugin.category,
            dependencies: release.dependencies.clone(),
            runtime_hash,
            runtime_javascript,
            stylesheet: String::new(),
            guidance: format!("{}{}", release.guidance, source_guidance),
            license: "LICENSES.txt".to_string(),
            license_text,
        };
        package.validate()?;
        Ok(package)
    }
}

impl CdnPagePluginSource {
    fn cached_or_builtin_registry(&self) -> SfumatoResult<PagePluginRegistry> {
        fs::read(&self.cache_path)
            .map_err(plugin_store_error)
            .and_then(|bytes| parse_registry(&bytes))
            .or_else(|_| parse_registry(BUILTIN_REGISTRY))
    }
}

/// Versioned plugin catalog stored under `~/.sfumato/plugins`.
#[derive(Clone, Debug)]
pub struct FilesystemPagePluginCatalog {
    root: PathBuf,
}

impl FilesystemPagePluginCatalog {
    /// Opens the production user plugin store.
    pub fn default_path() -> Result<Self> {
        Ok(Self {
            root: plugin_store_root()?,
        })
    }

    /// Opens a plugin store at an explicit root.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn current_version(&self, id: &str) -> SfumatoResult<String> {
        validate_plugin_id(id)?;
        let path = self.root.join(id).join("current");
        let version = fs::read_to_string(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SfumatoError::not_found(format!("Page plugin '{id}' is not installed"))
            } else {
                plugin_store_error(error)
            }
        })?;
        validate_version_component(version.trim())?;
        Ok(version.trim().to_string())
    }

    fn version_root(&self, id: &str, version: &str) -> SfumatoResult<PathBuf> {
        validate_plugin_id(id)?;
        validate_version_component(version)?;
        Ok(self.root.join(id).join("versions").join(version))
    }
}

impl PagePluginCatalog for FilesystemPagePluginCatalog {
    fn list(&self) -> SfumatoResult<Vec<PagePluginSummary>> {
        let mut plugins = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(plugins),
            Err(error) => return Err(plugin_store_error(error)),
        };
        for entry in entries {
            let entry = entry.map_err(plugin_store_error)?;
            if !entry.file_type().map_err(plugin_store_error)?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if validate_plugin_id(&id).is_ok() {
                plugins.push(self.load(&id)?.summary);
            }
        }
        plugins.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(plugins)
    }

    fn load(&self, id: &str) -> SfumatoResult<PagePluginPackage> {
        let version = self.current_version(id)?;
        let root = self.version_root(id, &version)?;
        reject_symlink(&root)?;
        let bytes = fs::read(root.join("package.json")).map_err(plugin_store_error)?;
        let package: DownloadedPagePluginPackage =
            serde_json::from_slice(&bytes).map_err(|error| {
                SfumatoError::config(format!("Invalid installed plugin '{id}': {error}"))
            })?;
        package.validate()?;
        if package.id != id || package.version != version {
            return Err(SfumatoError::config(format!(
                "Installed plugin '{id}' metadata does not match its selected version"
            )));
        }
        verify_hash(
            package.runtime_javascript.as_bytes(),
            &package.runtime_hash,
            "plugin runtime",
        )?;
        Ok(package.into_runtime_package())
    }

    fn install(&self, package: DownloadedPagePluginPackage) -> SfumatoResult<PagePluginPackage> {
        package.validate()?;
        verify_hash(
            package.runtime_javascript.as_bytes(),
            &package.runtime_hash,
            "plugin runtime",
        )?;
        let version_root = self.version_root(&package.id, &package.version)?;
        let parent = version_root
            .parent()
            .expect("version roots always have a parent");
        fs::create_dir_all(parent).map_err(plugin_store_error)?;
        reject_symlink(parent)?;
        let package_bytes = serde_json::to_vec_pretty(&package).map_err(|error| {
            SfumatoError::internal(format!("Could not serialize plugin package: {error}"))
        })?;
        write_atomic(&version_root.join("package.json"), &package_bytes)
            .map_err(plugin_store_error)?;
        let current = self.root.join(&package.id).join("current");
        write_atomic(&current, package.version.as_bytes()).map_err(plugin_store_error)?;
        Ok(package.into_runtime_package())
    }
}

fn plugin_store_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find the user home directory")?;
    Ok(home.join(".sfumato/plugins"))
}

fn parse_registry(bytes: &[u8]) -> SfumatoResult<PagePluginRegistry> {
    let registry: PagePluginRegistry = serde_json::from_slice(bytes)
        .map_err(|error| SfumatoError::config(format!("Invalid page plugin registry: {error}")))?;
    registry.validate()?;
    Ok(registry)
}

fn verify_hash(bytes: &[u8], expected: &str, label: &str) -> SfumatoResult<()> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected {
        return Err(SfumatoError::validation(format!(
            "{label} failed its SHA-256 integrity check"
        )));
    }
    Ok(())
}

fn validate_version_component(version: &str) -> SfumatoResult<()> {
    if version.is_empty()
        || version == "."
        || version == ".."
        || version.contains('/')
        || version.contains('\\')
    {
        return Err(SfumatoError::validation(format!(
            "Unsafe plugin version '{version}'"
        )));
    }
    Ok(())
}

fn validate_global_expression(value: &str) -> SfumatoResult<()> {
    if !value.starts_with("window.")
        || value.split('.').any(|segment| {
            segment.is_empty()
                || !segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    {
        return Err(SfumatoError::validation(format!(
            "Unsafe plugin browser global '{value}'"
        )));
    }
    Ok(())
}

fn tailwind_theme_source() -> &'static str {
    r#"@import "tailwindcss";
@theme inline {
  --color-background: var(--background);
  --color-foreground: var(--text);
  --color-card: var(--surface);
  --color-card-foreground: var(--text);
  --color-popover: var(--surface);
  --color-popover-foreground: var(--text);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--surface);
  --color-secondary: color-mix(in srgb, var(--surface) 85%, var(--text));
  --color-secondary-foreground: var(--text);
  --color-muted: color-mix(in srgb, var(--surface) 88%, var(--text));
  --color-muted-foreground: var(--muted);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--surface);
  --color-border: color-mix(in srgb, var(--muted) 35%, transparent);
  --color-input: color-mix(in srgb, var(--muted) 35%, transparent);
  --color-ring: var(--primary);
  --radius-sm: 0.25rem;
  --radius-md: 0.375rem;
  --radius-lg: 0.5rem;
}
@layer base {
  * { @apply border-border; }
  body { @apply bg-background text-foreground; }
}"#
}

fn reject_symlink(path: &Path) -> SfumatoResult<()> {
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(SfumatoError::validation(format!(
            "Plugin store path {} cannot be a symbolic link",
            path.display()
        )));
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Plugin store path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn plugin_transport_error(error: impl std::fmt::Display) -> SfumatoError {
    SfumatoError::provider(
        ErrorClass::Unavailable,
        format!("Plugin download failed: {error}"),
    )
}

fn plugin_store_error(error: impl std::fmt::Display) -> SfumatoError {
    SfumatoError::artifact(
        ErrorClass::Permanent,
        format!("Plugin store operation failed: {error}"),
    )
}

#[cfg(test)]
#[path = "../tests/unit/page_plugins.rs"]
mod tests;
