//! Schema-aware TOML persistence and platform configuration paths.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use sfumato_core::config::CONFIG_SCHEMA_VERSION;
use sfumato_core::repositories::RepositorySnapshot;
use sha2::{Digest, Sha256};

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
    Ok(read_versioned_snapshot(path, kind)?.value)
}

/// Reads one schema-v4 document with an optimistic concurrency token.
pub fn read_versioned_snapshot<T: DeserializeOwned>(
    path: &Path,
    kind: &str,
) -> Result<RepositorySnapshot<T>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Could not read {} config {}", kind, path.display()))?;
    let value = parse_versioned(&text, path, kind)?;
    Ok(RepositorySnapshot {
        value,
        revision: content_revision(&text),
    })
}

fn parse_versioned<T: DeserializeOwned>(text: &str, path: &Path, kind: &str) -> Result<T> {
    let value = toml::from_str::<toml::Value>(text)
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
    toml::from_str(text).with_context(|| format!("Could not parse {}", path.display()))
}

/// Reads an arbitrary TOML document without a Sfumato config schema check.
pub fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Could not parse {}", path.display()))
}

/// Atomically writes a pretty TOML document.
pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    with_write_lock(path, || write_toml_unlocked(path, value))
}

/// Atomically writes only when the persisted revision still matches.
pub fn write_toml_if_revision<T: Serialize>(
    path: &Path,
    value: &T,
    expected_revision: &str,
) -> Result<String> {
    with_write_lock(path, || {
        let current = if path.is_file() {
            content_revision(&fs::read_to_string(path).with_context(|| {
                format!(
                    "Could not read config {} for revision check",
                    path.display()
                )
            })?)
        } else {
            "missing".to_string()
        };
        if current != expected_revision {
            bail!(
                "Config {} changed since it was loaded; reload and retry the command",
                path.display()
            );
        }
        write_toml_unlocked(path, value)?;
        let rendered = fs::read_to_string(path)
            .with_context(|| format!("Could not read updated config {}", path.display()))?;
        Ok(content_revision(&rendered))
    })
}

/// Updates one TOML table while holding its cross-process write lock.
pub fn edit_toml(path: &Path, edit: impl FnOnce(&mut toml::Table) -> Result<()>) -> Result<()> {
    with_write_lock(path, || {
        let mut table = if path.is_file() {
            match read_toml::<toml::Value>(path)? {
                toml::Value::Table(table) => table,
                _ => bail!("Config file {} must contain a TOML table", path.display()),
            }
        } else {
            toml::Table::new()
        };
        edit(&mut table)?;
        write_toml_unlocked(path, &toml::Value::Table(table))
    })
}

fn with_write_lock<T>(path: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let parent = path
        .parent()
        .context("Config path must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("Could not create {}", parent.display()))?;
    let lock_path = lock_path(path);
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("Could not open config lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("Could not lock config {}", path.display()))?;
    let result = operation();
    let unlock_result = lock
        .unlock()
        .with_context(|| format!("Could not unlock config {}", path.display()));
    match (result, unlock_result) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

fn write_toml_unlocked<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .context("Config path must have a parent directory")?;
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
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("Could not sync config directory {}", parent.display()))?;
    Ok(())
}

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "config".into());
    name.push(".lock");
    path.with_file_name(name)
}

fn content_revision(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// Portable project-local configuration path.
pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".sfumato/project.toml")
}
