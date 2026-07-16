//! Filesystem persistence and validation for reusable theme packages.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sfumato_core::{
    errors::{ErrorClass, ErrorCode, SfumatoError, SfumatoResult},
    repositories::ThemeRepository,
    themes::{
        DEFAULT_THEME, THEME_SCHEMA_VERSION, ThemeManifest, ThemePackage, ThemeSummary,
        validate_theme_name,
    },
};

use crate::config_files::{ConfigPaths, read_toml, write_toml};

const HTML_CONTENT_SLOT: &str = "<!-- SFUMATO_CONTENT -->";

/// Filesystem-backed theme package repository.
#[derive(Clone, Debug)]
pub struct FilesystemThemeRepository {
    themes_dir: PathBuf,
}

impl FilesystemThemeRepository {
    /// Creates a repository rooted at an explicit directory.
    pub fn new(themes_dir: PathBuf) -> Self {
        Self { themes_dir }
    }

    /// Creates a repository at Sfumato's user-global theme directory.
    pub fn default_path() -> Result<Self> {
        Ok(Self::new(ConfigPaths::discover()?.themes))
    }

    fn names(&self) -> Result<Vec<String>> {
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

    fn resolve(&self, name: &str) -> Result<ThemePackage> {
        validate_theme_name(name)?;
        let root = self.themes_dir.join(name);
        let manifest_path = root.join("theme.toml");
        if !manifest_path.is_file() {
            bail!(
                "Theme '{name}' was not found in {}",
                self.themes_dir.display()
            );
        }
        let manifest: ThemeManifest = read_toml(&manifest_path)?;
        validate_manifest(&root, name, &manifest)?;
        Ok(ThemePackage { root, manifest })
    }
}

impl ThemeRepository for FilesystemThemeRepository {
    fn list(&self) -> SfumatoResult<Vec<ThemeSummary>> {
        theme_result((|| {
            Ok(self
                .names()?
                .into_iter()
                .map(|name| ThemeSummary { name })
                .collect())
        })())
    }

    fn load(&self, name: &str) -> SfumatoResult<ThemePackage> {
        theme_result(self.resolve(name))
    }

    fn create(&self, name: &str) -> SfumatoResult<ThemePackage> {
        theme_result((|| {
            validate_theme_name(name)?;
            self.install_default()?;
            let destination = self.themes_dir.join(name);
            if destination.exists() {
                bail!("Theme '{name}' already exists");
            }
            copy_dir(&self.themes_dir.join(DEFAULT_THEME), &destination)?;
            let mut manifest: ThemeManifest = read_toml(&destination.join("theme.toml"))?;
            manifest.name = name.to_string();
            manifest.description = format!("Custom Sfumato theme: {name}");
            write_toml(&destination.join("theme.toml"), &manifest)?;
            rewrite_marp_metadata(&destination.join(&manifest.adapters.marp_css), name)?;
            self.resolve(name)
        })())
    }

    fn install_default(&self) -> SfumatoResult<ThemePackage> {
        theme_result((|| {
            let root = self.themes_dir.join(DEFAULT_THEME);
            if !root.exists() {
                write_bundled_theme(&root)?;
            }
            self.resolve(DEFAULT_THEME)
        })())
    }
}

fn theme_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            return error.clone();
        }
        let message = format!("{error:#}");
        let code = if message.contains("was not found") {
            ErrorCode::NotFound
        } else {
            ErrorCode::Validation
        };
        SfumatoError::new(code, ErrorClass::Permanent, message)
    })
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

fn write_bundled_theme(root: &Path) -> Result<()> {
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
#[path = "../tests/unit/themes.rs"]
mod tests;
