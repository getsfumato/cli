use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{ProjectRegistry, load_project_config, themes_dir, write_toml};

pub const DEFAULT_THEME: &str = "sfumato-default";
pub const THEME_SCHEMA_VERSION: u32 = 1;
const HTML_CONTENT_SLOT: &str = "<!-- SFUMATO_CONTENT -->";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeManifest {
    pub schema_version: u32,
    pub name: String,
    pub description: String,
    pub tokens: ThemeTokens,
    pub adapters: ThemeAdapters,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeTokens {
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub fonts: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeAdapters {
    pub marp_css: PathBuf,
    #[serde(default)]
    pub html: Option<HtmlThemeAdapter>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HtmlThemeAdapter {
    pub shell: PathBuf,
    pub css: PathBuf,
    #[serde(default)]
    pub script: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ThemePackage {
    pub root: PathBuf,
    pub manifest: ThemeManifest,
}

impl ThemePackage {
    pub fn marp_css_path(&self) -> PathBuf {
        self.root.join(&self.manifest.adapters.marp_css)
    }
}

#[derive(Debug)]
pub struct ThemeService {
    themes_dir: PathBuf,
}

impl ThemeService {
    pub fn load() -> Result<Self> {
        Ok(Self {
            themes_dir: themes_dir().context("Could not find Sfumato themes directory")?,
        })
    }

    #[cfg(test)]
    fn load_from(themes_dir: PathBuf) -> Self {
        Self { themes_dir }
    }

    pub fn install_default(&self) -> Result<()> {
        let root = self.themes_dir.join(DEFAULT_THEME);
        if root.exists() {
            return Ok(());
        }
        write_bundled_theme(&root, DEFAULT_THEME)
    }

    pub fn create(&self, name: &str) -> Result<()> {
        validate_theme_name(name)?;
        self.install_default()?;
        let destination = self.themes_dir.join(name);
        if destination.exists() {
            bail!("Theme '{name}' already exists");
        }
        copy_dir(&self.themes_dir.join(DEFAULT_THEME), &destination)?;
        let mut manifest: ThemeManifest =
            crate::config::read_toml(&destination.join("theme.toml"))?;
        manifest.name = name.to_string();
        manifest.description = format!("Custom Sfumato theme: {name}");
        write_toml(&destination.join("theme.toml"), &manifest)?;
        rewrite_marp_metadata(&destination.join(&manifest.adapters.marp_css), name)?;
        self.resolve(name)?;
        println!("Created theme '{name}' at {}", destination.display());
        Ok(())
    }

    pub fn list(&self) -> Result<()> {
        for name in self.names()? {
            println!("{name}");
        }
        Ok(())
    }

    pub fn names(&self) -> Result<Vec<String>> {
        if !self.themes_dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = fs::read_dir(&self.themes_dir)
            .with_context(|| format!("Could not read {}", self.themes_dir.display()))?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join("theme.toml").is_file())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    pub fn show(&self, name: &str) -> Result<String> {
        let package = self.resolve(name)?;
        toml::to_string_pretty(&package.manifest).context("Could not render theme manifest")
    }

    pub fn use_for_project(&self, name: &str, requested_project: Option<&str>) -> Result<()> {
        let registry = ProjectRegistry::load()?;
        self.use_for_project_in_registry(name, requested_project, &registry)
    }

    fn use_for_project_in_registry(
        &self,
        name: &str,
        requested_project: Option<&str>,
        registry: &ProjectRegistry,
    ) -> Result<()> {
        self.resolve(name)?;
        let (_, root) = registry.selected(requested_project)?;
        let path = crate::config::project_config_path(&root);
        let mut project = load_project_config(&path, DEFAULT_THEME)?;
        project.theme = name.to_string();
        write_toml(&path, &project)?;
        println!("Project '{}' now uses theme '{name}'", project.name);
        Ok(())
    }

    pub fn resolve(&self, name: &str) -> Result<ThemePackage> {
        validate_theme_name(name)?;
        let root = self.themes_dir.join(name);
        let manifest_path = root.join("theme.toml");
        if !manifest_path.is_file() {
            bail!(
                "Theme '{name}' was not found in {}",
                self.themes_dir.display()
            );
        }
        let manifest: ThemeManifest = crate::config::read_toml(&manifest_path)?;
        validate_manifest(&root, name, &manifest)?;
        Ok(ThemePackage { root, manifest })
    }
}

fn validate_theme_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || name.starts_with('-')
        || name.ends_with('-')
    {
        bail!("Invalid theme name '{name}'. Use lowercase letters, numbers, and hyphens.");
    }
    Ok(())
}

fn validate_manifest(root: &Path, requested_name: &str, manifest: &ThemeManifest) -> Result<()> {
    if manifest.schema_version != THEME_SCHEMA_VERSION {
        bail!(
            "Theme '{}' uses unsupported schema version {}",
            manifest.name,
            manifest.schema_version
        );
    }
    if manifest.name != requested_name {
        bail!(
            "Theme directory '{requested_name}' does not match manifest name '{}'",
            manifest.name
        );
    }
    validate_adapter_file(root, &manifest.adapters.marp_css, "Marp CSS")?;
    let marp_css = fs::read_to_string(root.join(&manifest.adapters.marp_css))
        .context("Could not read theme Marp CSS")?;
    if !marp_css.contains(&format!("/* @theme {} */", manifest.name)) {
        bail!(
            "Theme '{}' Marp CSS must include `/* @theme {} */`",
            manifest.name,
            manifest.name
        );
    }
    if let Some(html) = &manifest.adapters.html {
        validate_adapter_file(root, &html.shell, "HTML shell")?;
        validate_adapter_file(root, &html.css, "HTML CSS")?;
        if let Some(script) = &html.script {
            validate_adapter_file(root, script, "HTML script")?;
        }
        let shell =
            fs::read_to_string(root.join(&html.shell)).context("Could not read HTML shell")?;
        if shell.matches(HTML_CONTENT_SLOT).count() != 1 {
            bail!(
                "Theme '{}' HTML shell must contain exactly one {HTML_CONTENT_SLOT}",
                manifest.name
            );
        }
    }
    Ok(())
}

fn validate_adapter_file(root: &Path, relative: &Path, label: &str) -> Result<()> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "{label} path '{}' must stay inside the theme package",
            relative.display()
        );
    }
    if !root.join(relative).is_file() {
        bail!(
            "{label} file '{}' does not exist",
            root.join(relative).display()
        );
    }
    Ok(())
}

fn write_bundled_theme(root: &Path, name: &str) -> Result<()> {
    for (relative, contents) in [
        (
            "theme.toml",
            include_str!("../assets/themes/sfumato-default/theme.toml"),
        ),
        (
            "marp/theme.css",
            include_str!("../assets/themes/sfumato-default/marp/theme.css"),
        ),
        (
            "html/page.html",
            include_str!("../assets/themes/sfumato-default/html/page.html"),
        ),
        (
            "html/style.css",
            include_str!("../assets/themes/sfumato-default/html/style.css"),
        ),
        (
            "html/script.js",
            include_str!("../assets/themes/sfumato-default/html/script.js"),
        ),
    ] {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    if name != DEFAULT_THEME {
        bail!("Bundled theme may only be installed as '{DEFAULT_THEME}'");
    }
    Ok(())
}

fn copy_dir(source: &Path, destination: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn rewrite_marp_metadata(path: &Path, name: &str) -> Result<()> {
    let css = fs::read_to_string(path)?;
    let rewritten = css.replacen(
        &format!("/* @theme {DEFAULT_THEME} */"),
        &format!("/* @theme {name} */"),
        1,
    );
    fs::write(path, rewritten)?;
    Ok(())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/themes.rs"]
mod tests;
