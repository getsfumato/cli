use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use toml::{Table, Value};

use crate::{
    cli::ConfigScope,
    config::{
        ConfigOverrides, EffectiveConfig, ProjectRegistry, project_config_path, user_config_path,
    },
};

#[derive(Debug)]
pub struct ConfigService {
    user_config_path: PathBuf,
}

impl ConfigService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            user_config_path: user_config_path()
                .context("Could not find a user configuration directory")?,
        })
    }

    pub fn show(
        &self,
        scope: ConfigScope,
        project: Option<String>,
        key: Option<String>,
    ) -> Result<String> {
        let value = match scope {
            ConfigScope::Effective => {
                let config = EffectiveConfig::load(ConfigOverrides {
                    project,
                    ..Default::default()
                })?;
                Value::try_from(config).context("Could not serialize effective config")?
            }
            ConfigScope::User => {
                crate::config::GlobalConfig::load()?;
                Value::Table(self.read_config_table(&self.user_config_path)?)
            }
            ConfigScope::Project => {
                Value::Table(self.read_config_table(&self.project_path(project.as_deref())?)?)
            }
        };
        let shown = if let Some(key) = key {
            get_dotted_value(&value, &key)
                .with_context(|| format!("Config key '{key}' was not found"))?
                .clone()
        } else {
            value
        };
        render_config_value(&shown)
    }

    pub fn set(
        &self,
        scope: ConfigScope,
        project: Option<String>,
        key: &str,
        raw_value: &str,
    ) -> Result<()> {
        let path = self.editable_path(scope, project.as_deref())?;
        let mut table = self.read_config_table(&path)?;
        set_dotted_value(&mut table, key, parse_config_value(raw_value))?;
        self.write_config_table(&path, &table)?;
        println!("Set {key} in {}", path.display());
        Ok(())
    }

    pub fn delete(&self, scope: ConfigScope, project: Option<String>, key: &str) -> Result<()> {
        let path = self.editable_path(scope, project.as_deref())?;
        let mut table = self.read_config_table(&path)?;
        delete_dotted_value(&mut table, key)?;
        self.write_config_table(&path, &table)?;
        println!("Deleted {key} from {}", path.display());
        Ok(())
    }

    fn editable_path(&self, scope: ConfigScope, project: Option<&str>) -> Result<PathBuf> {
        match scope {
            ConfigScope::User => {
                crate::config::GlobalConfig::load()?;
                Ok(self.user_config_path.clone())
            }
            ConfigScope::Project => self.project_path(project),
            ConfigScope::Effective => bail!(
                "The effective config is merged and read-only. Use --scope user or --scope project."
            ),
        }
    }

    fn project_path(&self, requested: Option<&str>) -> Result<PathBuf> {
        let registry = ProjectRegistry::load()?;
        let (_, root) = registry.selected(requested)?;
        let path = project_config_path(&root);
        crate::config::load_project_config(&path, crate::themes::DEFAULT_THEME)?;
        Ok(path)
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

    fn write_config_table(&self, path: &Path, table: &Table) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }
        let rendered = toml::to_string_pretty(table).context("Could not render config as TOML")?;
        fs::write(path, rendered).with_context(|| format!("Could not write {}", path.display()))
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
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/config_editor.rs"]
mod tests;
