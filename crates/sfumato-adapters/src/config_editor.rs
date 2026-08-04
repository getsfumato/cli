//! TOML-backed generic configuration editor.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use sfumato_core::{
    application::EffectiveConfigResolver,
    config::ConfigOverrides,
    config_editor::{ConfigEditor, ConfigTarget},
    errors::{SfumatoError, SfumatoResult},
    repositories::{GlobalConfigRepository, ProjectRepository},
};
use toml::{Table, Value};

use crate::{
    config_dto::{GlobalConfigDto, ProjectConfigDto},
    config_files::{edit_toml, project_config_path},
};

/// Schema-aware TOML editor for production configuration files.
pub struct TomlConfigEditor {
    user_config_path: PathBuf,
    global: Arc<dyn GlobalConfigRepository>,
    projects: Arc<dyn ProjectRepository>,
    effective: Arc<dyn EffectiveConfigResolver>,
}

impl TomlConfigEditor {
    /// Creates an editor over schema-aware repositories and effective resolution.
    pub fn new(
        user_config_path: PathBuf,
        global: Arc<dyn GlobalConfigRepository>,
        projects: Arc<dyn ProjectRepository>,
        effective: Arc<dyn EffectiveConfigResolver>,
    ) -> Self {
        Self {
            user_config_path,
            global,
            projects,
            effective,
        }
    }

    fn editable_path(&self, scope: ConfigTarget, project: Option<&str>) -> Result<PathBuf> {
        match scope {
            ConfigTarget::User => {
                self.global.load()?;
                Ok(self.user_config_path.clone())
            }
            ConfigTarget::Project => self.project_path(project),
            ConfigTarget::Effective => bail!(
                "The effective config is merged and read-only. Use --scope user or --scope project."
            ),
        }
    }

    fn project_path(&self, requested: Option<&str>) -> Result<PathBuf> {
        let registry = self.projects.registry()?;
        let (name, root) = registry.selected(requested)?;
        self.projects.load(Some(&name))?;
        Ok(project_config_path(&root))
    }

    fn read_config_table(&self, path: &Path) -> Result<Table> {
        if !path.exists() {
            return Ok(Table::new());
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("Could not read config file {}", path.display()))?;
        match toml::from_str::<Value>(&text)
            .with_context(|| format!("Could not parse config file {}", path.display()))?
        {
            Value::Table(table) => Ok(table),
            _ => bail!("Config file {} must contain a TOML table", path.display()),
        }
    }

    fn validate_table(&self, scope: ConfigTarget, table: &Table) -> Result<()> {
        let value = Value::Table(table.clone());
        match scope {
            ConfigTarget::User => {
                let persisted: GlobalConfigDto = value
                    .try_into()
                    .context("The change would make the user config invalid")?;
                persisted.into_domain().map(|_| ())
            }
            ConfigTarget::Project => {
                let persisted: ProjectConfigDto = value
                    .try_into()
                    .context("The change would make the project config invalid")?;
                let project = persisted.into_domain()?;
                let global = self.global.load()?;
                for profile in project
                    .model_defaults
                    .values()
                    .chain(project.model_roles.values())
                {
                    if !global.models.contains_key(profile) {
                        bail!("Project references unknown model profile '{profile}'");
                    }
                }
                Ok(())
            }
            ConfigTarget::Effective => bail!("Effective configuration is read-only"),
        }
    }
}

impl ConfigEditor for TomlConfigEditor {
    fn show(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: Option<String>,
    ) -> SfumatoResult<String> {
        config_result((|| {
            let mut value = match scope {
                ConfigTarget::Effective => {
                    let config = self.effective.resolve(ConfigOverrides {
                        project,
                        ..Default::default()
                    })?;
                    Value::try_from(config).context("Could not serialize effective config")?
                }
                ConfigTarget::User => {
                    self.global.load()?;
                    Value::Table(self.read_config_table(&self.user_config_path)?)
                }
                ConfigTarget::Project => {
                    Value::Table(self.read_config_table(&self.project_path(project.as_deref())?)?)
                }
            };
            redact_sensitive_values(&mut value);
            let shown = if let Some(key) = key {
                get_dotted_value(&value, &key)
                    .with_context(|| format!("Config key '{key}' was not found"))?
                    .clone()
            } else {
                value
            };
            render_config_value(&shown)
        })())
    }

    fn set(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: &str,
        raw_value: &str,
    ) -> SfumatoResult<PathBuf> {
        config_result((|| {
            let path = self.editable_path(scope, project.as_deref())?;
            reject_secret_key(key)?;
            edit_toml(&path, |table| {
                set_dotted_value(table, key, parse_config_value(raw_value))?;
                self.validate_table(scope, table)
            })?;
            Ok(path)
        })())
    }

    fn delete(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: &str,
    ) -> SfumatoResult<PathBuf> {
        config_result((|| {
            let path = self.editable_path(scope, project.as_deref())?;
            // Deliberately not guarded by `reject_secret_key`: the guard exists
            // to keep credentials out of the config, and deleting one is the
            // remediation. Rejecting it would leave a key written by an earlier
            // release with no way out of the file but a hand edit.
            edit_toml(&path, |table| {
                delete_dotted_value(table, key)?;
                self.validate_table(scope, table)
            })?;
            Ok(path)
        })())
    }
}

fn config_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| SfumatoError::config(format_args!("{error:#}")))
}

/// Names that hold a credential rather than a reference to one.
///
/// Matched as whole underscore-separated words so that a real header spelling
/// (`x-api-key`, `xi-api-key`) is caught while a tuning knob that merely
/// contains one of these words (`max_tokens`) is not.
const SECRET_NAMES: &[&str] = &[
    "api_key",
    "apikey",
    "authorization",
    "access_key",
    "auth_token",
    "bearer",
    "cookie",
    "passwd",
    "password",
    "private_key",
    "secret",
    "session_key",
    "token",
];

/// Folds a config key segment or header name to one spelling.
///
/// `x-api-key`, `X_API_KEY` and `x-Api-Key` are the same name written three
/// ways; every secret decision is made on this form so the write guard and the
/// read redactor cannot disagree about what a secret looks like.
fn normalise_key_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

/// Reports whether a config key segment names a credential value.
///
/// The single source of truth behind both `reject_secret_key` (write) and
/// `redact_sensitive_values` (read): a name added here is protected on both
/// paths at once.
fn is_secret_key_name(name: &str) -> bool {
    let normalised = normalise_key_name(name);
    // A credential *reference* (`credential = "stored:connector/x"`) is a
    // pointer, not a secret, and has to stay readable and editable.
    if normalised == "credential" {
        return false;
    }
    let words: Vec<&str> = normalised
        .split('_')
        .filter(|word| !word.is_empty())
        .collect();
    // Match any contiguous run of whole words, so `x_api_key` contains the
    // word pair `api_key` but `max_tokens` never yields the word `token`.
    (0..words.len()).any(|start| {
        (start..words.len()).any(|end| {
            let phrase = words[start..=end].join("_");
            SECRET_NAMES.contains(&phrase.as_str())
        })
    })
}

fn reject_secret_key(key: &str) -> Result<()> {
    if split_key(key).iter().any(|part| is_secret_key_name(part)) {
        bail!(
            "Secrets cannot be edited through generic config commands; configure a credential reference instead"
        );
    }
    Ok(())
}

fn redact_sensitive_values(value: &mut Value) {
    let Some(table) = value.as_table_mut() else {
        return;
    };
    for (key, value) in table {
        if is_secret_key_name(key) {
            *value = Value::String("[REDACTED]".to_string());
        } else {
            redact_sensitive_values(value);
        }
    }
}

fn parse_config_value(raw: &str) -> Value {
    raw.parse::<Value>()
        .unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn render_config_value(value: &Value) -> Result<String> {
    match value {
        Value::Table(_) => toml::to_string_pretty(value).context("Could not render config as TOML"),
        _ => Ok(value.to_string()),
    }
}

fn get_dotted_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in split_key(key) {
        current = current.get(part)?;
    }
    Some(current)
}

fn set_dotted_value(table: &mut Table, key: &str, value: Value) -> Result<()> {
    let parts = split_key(key);
    if parts.is_empty() {
        bail!("Config key cannot be empty");
    }
    let mut current = table;
    for part in &parts[..parts.len() - 1] {
        let entry = current
            .entry((*part).to_string())
            .or_insert_with(|| Value::Table(Table::new()));
        current = entry
            .as_table_mut()
            .with_context(|| format!("Config key '{part}' is not a table"))?;
    }
    current.insert(parts[parts.len() - 1].to_string(), value);
    Ok(())
}

fn delete_dotted_value(table: &mut Table, key: &str) -> Result<()> {
    let parts = split_key(key);
    if parts.is_empty() {
        bail!("Config key cannot be empty");
    }
    let mut current = table;
    for part in &parts[..parts.len() - 1] {
        current = current
            .get_mut(*part)
            .and_then(Value::as_table_mut)
            .with_context(|| format!("Config key '{key}' was not found"))?;
    }
    if current.remove(parts[parts.len() - 1]).is_none() {
        bail!("Config key '{key}' was not found");
    }
    Ok(())
}

fn split_key(key: &str) -> Vec<&str> {
    key.split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/config_editor.rs"]
mod tests;
