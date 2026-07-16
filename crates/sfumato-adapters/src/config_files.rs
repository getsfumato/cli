//! Schema-aware TOML persistence and platform configuration paths.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};
use sfumato_core::config::CONFIG_SCHEMA_VERSION;

/// User-global paths owned by the filesystem adapter.
#[derive(Clone, Debug)]
pub struct ConfigPaths {
    pub user_config: PathBuf,
    pub project_registry: PathBuf,
    pub themes: PathBuf,
}

impl ConfigPaths {
    /// Discovers Sfumato's platform-specific configuration paths.
    pub fn discover() -> Result<Self> {
        let config = dirs::config_dir().context("Could not find user configuration directory")?;
        let root = config.join("sfumato");
        Ok(Self {
            user_config: root.join("config.toml"),
            project_registry: root.join("projects.toml"),
            themes: root.join("themes"),
        })
    }
}

/// Reads and validates a schema-v4 TOML document.
pub fn read_versioned<T: DeserializeOwned>(path: &Path, kind: &str) -> Result<T> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Could not read {} config {}", kind, path.display()))?;
    let value = toml::from_str::<toml::Value>(&text)
        .with_context(|| format!("Could not parse {} config {}", kind, path.display()))?;
    let version = value
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .with_context(|| format!("{kind} config {} is missing schema_version", path.display()))?;
    if version != i64::from(CONFIG_SCHEMA_VERSION) {
        bail!(
            "Unsupported {kind} config schema {version} at {}; Sfumato v0.2 requires schema {}. Reinitialize the configuration instead of migrating it in place.",
            path.display(),
            CONFIG_SCHEMA_VERSION
        );
    }
    toml::from_str(&text).with_context(|| format!("Could not parse {}", path.display()))
}

/// Reads an arbitrary TOML document without a Sfumato config schema check.
pub fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Could not parse {}", path.display()))
}

/// Atomically writes a pretty TOML document.
pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .context("Config path must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("Could not create {}", parent.display()))?;
    let rendered = toml::to_string_pretty(value).context("Could not render config as TOML")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "Could not create a temporary config in {}",
            parent.display()
        )
    })?;
    temporary
        .write_all(rendered.as_bytes())
        .with_context(|| format!("Could not write temporary config for {}", path.display()))?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Could not atomically replace {}", path.display()))?;
    Ok(())
}

/// Portable project-local configuration path.
pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".sfumato/project.toml")
}
