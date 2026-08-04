//! Filesystem persistence and validation for reusable theme packages.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
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
        restore_missing_document_css(&root, &manifest)?;
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
            if root.exists() {
                repair_bundled_theme(&root)?;
            } else {
                write_bundled_theme(&root)?;
            }
            self.resolve(DEFAULT_THEME)
        })())
    }

    fn import_design(&self, path: PathBuf, name: Option<&str>) -> SfumatoResult<ThemePackage> {
        theme_result((|| {
            let source = fs::read_to_string(&path)
                .with_context(|| format!("Could not read DESIGN.md {}", path.display()))?;
            let design = parse_design_document(&source)?;
            let package_name = name
                .map(str::to_owned)
                .unwrap_or_else(|| design_name_slug(&design.name));
            validate_theme_name(&package_name)?;
            self.install_default()?;
            let destination = self.themes_dir.join(&package_name);
            if destination.exists() {
                bail!("Theme '{package_name}' already exists");
            }
            copy_dir(&self.themes_dir.join(DEFAULT_THEME), &destination)?;
            let mut manifest: ThemeManifest = read_toml(&destination.join("theme.toml"))?;
            manifest.name = package_name.clone();
            manifest.description = design
                .description
                .clone()
                .unwrap_or_else(|| format!("Imported from {}", path.display()));
            manifest.tokens.colors = design.colors.clone();
            apply_design_typography(&mut manifest, &design);
            write_toml(&destination.join("theme.toml"), &manifest)?;
            fs::write(destination.join("DESIGN.md"), source)?;
            write_design_adapters(&destination, &manifest)?;
            self.resolve(&package_name)
        })())
    }

    fn export_design(&self, name: &str, path: PathBuf) -> SfumatoResult<PathBuf> {
        theme_result((|| {
            let theme = self.resolve(name)?;
            let output = if path.is_dir() {
                path.join("DESIGN.md")
            } else {
                path
            };
            let parent = output
                .parent()
                .context("DESIGN.md output path has no parent")?;
            fs::create_dir_all(parent)?;
            let rendered = render_design_document(&theme.manifest)?;
            let temporary = tempfile::NamedTempFile::new_in(parent)?;
            fs::write(temporary.path(), rendered)?;
            temporary.persist(&output).map_err(|error| error.error)?;
            Ok(output)
        })())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct DesignDocument {
    #[serde(default = "design_version")]
    version: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    colors: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    typography: BTreeMap<String, DesignTypography>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesignTypography {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_weight: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line_height: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    letter_spacing: Option<String>,
}

fn design_version() -> String {
    "alpha".into()
}

fn parse_design_document(source: &str) -> Result<DesignDocument> {
    let normalized = source.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .context("DESIGN.md must start with YAML frontmatter")?;
    let (frontmatter, body) = rest
        .split_once("\n---\n")
        .context("DESIGN.md frontmatter is not closed")?;
    reject_duplicate_design_sections(body)?;
    let design: DesignDocument =
        serde_yaml::from_str(frontmatter).context("Could not parse DESIGN.md design tokens")?;
    if design.version != "alpha" {
        bail!(
            "Unsupported DESIGN.md version '{}'; expected alpha",
            design.version
        );
    }
    if design.name.trim().is_empty() {
        bail!("DESIGN.md name cannot be empty");
    }
    if design.colors.is_empty() {
        bail!("DESIGN.md must define at least one color token");
    }
    for (name, color) in &design.colors {
        validate_design_color(name, color)?;
    }
    Ok(design)
}

fn reject_duplicate_design_sections(body: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for heading in body.lines().filter_map(|line| line.strip_prefix("## ")) {
        let heading = heading.trim().to_ascii_lowercase();
        let canonical = match heading.as_str() {
            "overview" | "brand & style" => Some("overview"),
            "colors" => Some("colors"),
            "typography" => Some("typography"),
            "layout" | "layout & spacing" => Some("layout"),
            "elevation & depth" | "elevation" => Some("elevation"),
            "shapes" => Some("shapes"),
            "components" => Some("components"),
            "do's and don'ts" => Some("do's and don'ts"),
            _ => None,
        };
        if canonical.is_some_and(|section| !seen.insert(section)) {
            bail!("DESIGN.md contains duplicate section heading '## {heading}'");
        }
    }
    Ok(())
}

fn validate_design_color(name: &str, value: &str) -> Result<()> {
    validate_hex_colour("DESIGN.md color", name, value)
}

/// Checks one sRGB hex colour token.
///
/// Shared by the DESIGN.md import and the theme manifest so a value that one
/// path rejects cannot enter through the other. `is_ascii_hexdigit` is what
/// makes this safe as well as correct: consumers parse these values by
/// byte-slicing, which panics on a multi-byte character.
fn validate_hex_colour(label: &str, name: &str, value: &str) -> Result<()> {
    let hex = value
        .strip_prefix('#')
        .with_context(|| format!("{label} '{name}' must be an sRGB hex value beginning with #"))?;
    if !matches!(hex.len(), 3 | 4 | 6 | 8)
        || !hex.chars().all(|character| character.is_ascii_hexdigit())
    {
        bail!("{label} '{name}' has invalid hex value '{value}'");
    }
    Ok(())
}

fn design_name_slug(name: &str) -> String {
    let mut slug = name
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug.trim_matches('-').to_string()
}

fn apply_design_typography(manifest: &mut ThemeManifest, design: &DesignDocument) {
    let font = |names: &[&str]| {
        names.iter().find_map(|name| {
            design
                .typography
                .get(*name)
                .and_then(|entry| entry.font_family.clone())
        })
    };
    if let Some(heading) = font(&["h1", "headline-display", "headline-lg", "heading"]) {
        manifest.tokens.fonts.insert("heading".into(), heading);
    }
    if let Some(body) = font(&["body", "body-md", "body-lg"]) {
        manifest.tokens.fonts.insert("body".into(), body);
    }
}

fn write_design_adapters(root: &Path, manifest: &ThemeManifest) -> Result<()> {
    let color = |name: &str, fallback: &str| {
        manifest
            .tokens
            .colors
            .get(name)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    let background = color("background", &color("neutral", "#ffffff"));
    let surface = color("surface", &background);
    let text = color("text", &color("on-surface", "#202124"));
    let muted = color("muted", &color("secondary", &text));
    let primary = color("primary", &text);
    let accent = color("accent", &color("tertiary", &primary));
    let body = manifest
        .tokens
        .fonts
        .get("body")
        .map(String::as_str)
        .unwrap_or("Inter, Arial, sans-serif");
    let heading = manifest
        .tokens
        .fonts
        .get("heading")
        .map(String::as_str)
        .unwrap_or(body);
    let marp = format!(
        "/* @theme {} */\n\n@import \"default\";\n\n:root {{\n  --sfumato-background: {background};\n  --sfumato-surface: {surface};\n  --sfumato-text: {text};\n  --sfumato-muted: {muted};\n  --sfumato-primary: {primary};\n  --sfumato-accent: {accent};\n}}\n\nsection {{ background: var(--sfumato-background); color: var(--sfumato-text); font-family: {body}; }}\nh1, h2, h3 {{ color: var(--sfumato-primary); font-family: {heading}; }}\na, strong {{ color: var(--sfumato-accent); }}\ncode {{ background: var(--sfumato-surface); }}\n",
        manifest.name
    );
    fs::write(root.join(&manifest.adapters.marp_css), marp)?;
    if let Some(html) = &manifest.adapters.html {
        let css = format!(
            ":root {{ color-scheme: light dark; --background: {background}; --surface: {surface}; --text: {text}; --muted: {muted}; --primary: {primary}; --accent: {accent}; }}\nbody {{ margin: 0; background: var(--background); color: var(--text); font-family: {body}; }}\nh1, h2, h3 {{ font-family: {heading}; }}\nmain {{ max-width: 72rem; margin: 0 auto; padding: 2rem; }}\n"
        );
        fs::write(root.join(&html.css), css)?;
    }
    Ok(())
}

fn render_design_document(manifest: &ThemeManifest) -> Result<String> {
    let body_font = manifest.tokens.fonts.get("body").cloned();
    let heading_font = manifest.tokens.fonts.get("heading").cloned();
    let mut typography = BTreeMap::new();
    if let Some(font_family) = heading_font {
        typography.insert(
            "h1".into(),
            DesignTypography {
                font_family: Some(font_family),
                ..Default::default()
            },
        );
    }
    if let Some(font_family) = body_font {
        typography.insert(
            "body-md".into(),
            DesignTypography {
                font_family: Some(font_family),
                ..Default::default()
            },
        );
    }
    let design = DesignDocument {
        version: design_version(),
        name: manifest.name.clone(),
        description: Some(manifest.description.clone()),
        colors: manifest.tokens.colors.clone(),
        typography,
    };
    let yaml = serde_yaml::to_string(&design)?
        .trim_start_matches("---\n")
        .to_string();
    Ok(format!(
        "---\n{yaml}---\n\n# {}\n\n## Overview\n\n{}\n\n## Colors\n\nUse the declared semantic color tokens consistently across generated resources.\n\n## Typography\n\nUse the heading and body families declared in the normative tokens.\n\n## Do's and Don'ts\n\n- Do preserve semantic contrast and visual hierarchy.\n- Don't introduce unrelated palettes or typography.\n",
        manifest.name, manifest.description
    ))
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
    // `tokens.colors` is a free-form map that nothing validated, and consumers
    // parse the values by byte-slicing: an accented typo — plausible when
    // writing in Spanish — panicked the chart tool. Validating here also stops
    // the quieter failure, where an unparseable colour was treated as light and
    // silently turned a dark theme's chart light.
    for (name, value) in &manifest.tokens.colors {
        validate_hex_colour("Theme colour", name, value)?;
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
    // A theme with no `[adapters.document]` is valid: the document renderer
    // falls back to a bundled stylesheet. Declaring one and not shipping it is
    // not, and must surface here rather than mid-render, after a paid model call.
    if let Some(document) = &manifest.adapters.document {
        validate_adapter_file(root, &document.css, "Document print CSS")?;
        if let Some(cover_image) = &document.cover_image {
            validate_adapter_file(root, cover_image, "Document cover image")?;
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

/// Every file the bundled default theme is made of.
///
/// This list must cover each path `assets/themes/sfumato-default/theme.toml`
/// declares under `[adapters]`; a declared file that is missing here installs a
/// theme that only fails once a renderer reaches for it.
const BUNDLED_THEME_FILES: &[(&str, &str)] = &[
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
    (
        "document/print.css",
        include_str!("../assets/themes/sfumato-default/document/print.css"),
    ),
];

fn write_bundled_theme(root: &Path) -> Result<()> {
    for (relative, contents) in BUNDLED_THEME_FILES {
        write_bundled_theme_file(root, relative, contents)?;
    }
    Ok(())
}

/// Restores bundled files that are missing from an installed default theme.
///
/// An install from a release that shipped an incomplete file list leaves a
/// default theme that declares adapters it does not contain. Writing only the
/// absent files repairs it without discarding edits the user made to the rest.
fn repair_bundled_theme(root: &Path) -> Result<()> {
    for (relative, contents) in BUNDLED_THEME_FILES {
        if !root.join(relative).is_file() {
            write_bundled_theme_file(root, relative, contents)?;
        }
    }
    Ok(())
}

/// Restores a declared-but-absent document stylesheet from the bundled one.
///
/// Every theme installed or created by a release whose bundled file list omitted
/// `document/print.css` declares the adapter without shipping it — including
/// custom themes, which are copied from the default. Without this, tightening
/// validation would stop those themes loading at all, which is worse than the
/// late render failure it replaces. Repairing is possible because the file is
/// theme-independent: it styles print layout from the theme's own tokens.
fn restore_missing_document_css(root: &Path, manifest: &ThemeManifest) -> Result<()> {
    let Some(document) = &manifest.adapters.document else {
        return Ok(());
    };
    // Leave an unsafe path to `validate_adapter_file`, which rejects it with a
    // clear message rather than writing outside the package.
    if document.css.is_absolute()
        || document.css.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Ok(());
    }
    let path = root.join(&document.css);
    if path.is_file() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &path,
        include_str!("../assets/themes/sfumato-default/document/print.css"),
    )
    .with_context(|| format!("Could not restore theme print CSS {}", path.display()))?;
    Ok(())
}

fn write_bundled_theme_file(root: &Path, relative: &str, contents: &str) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
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
