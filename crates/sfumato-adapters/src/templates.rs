//! Filesystem packages for reusable generation templates.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sfumato_core::{
    errors::{ErrorClass, ErrorCode, SfumatoError, SfumatoResult},
    templates::{
        GenerationTemplate, GenerationTemplateCatalog, GenerationTemplateManifest,
        GenerationTemplateSummary, TEMPLATE_CONTENT_SLOT, TEMPLATE_SCHEMA_VERSION, TemplateKind,
        validate_template_name, validate_template_source,
    },
};

use crate::config_files::{ConfigPaths, read_toml, write_toml};

/// User-global filesystem template catalog.
#[derive(Clone, Debug)]
pub struct FilesystemGenerationTemplateCatalog {
    root: PathBuf,
}

impl FilesystemGenerationTemplateCatalog {
    /// Creates a catalog rooted at an explicit directory.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Creates the production catalog under Sfumato's config directory.
    pub fn default_path() -> Result<Self> {
        Ok(Self::new(ConfigPaths::discover()?.templates))
    }

    fn resolve(
        &self,
        name: &str,
        expected_kind: Option<TemplateKind>,
    ) -> Result<GenerationTemplate> {
        validate_template_name(name)?;
        let root = self.root.join(name);
        let manifest_path = root.join("template.toml");
        if !manifest_path.is_file() {
            bail!(
                "Generation template '{name}' was not found in {}",
                self.root.display()
            );
        }
        let manifest: GenerationTemplateManifest = read_toml(&manifest_path)?;
        validate_manifest(&root, name, expected_kind, &manifest)?;
        let source = fs::read_to_string(root.join(&manifest.source))
            .with_context(|| format!("Could not read generation template '{name}'"))?;
        validate_template_source(&source)?;
        Ok(GenerationTemplate {
            root,
            manifest,
            source,
        })
    }
}

impl GenerationTemplateCatalog for FilesystemGenerationTemplateCatalog {
    fn list(&self, kind: Option<TemplateKind>) -> SfumatoResult<Vec<GenerationTemplateSummary>> {
        template_result((|| {
            if !self.root.exists() {
                return Ok(Vec::new());
            }
            let mut names = fs::read_dir(&self.root)
                .with_context(|| format!("Could not read {}", self.root.display()))?
                .filter_map(Result::ok)
                .filter(|entry| entry.path().join("template.toml").is_file())
                .map(|entry| {
                    entry
                        .file_name()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("Template directory name must be valid UTF-8"))
                })
                .collect::<Result<Vec<_>>>()?;
            names.sort();
            let mut templates = names
                .into_iter()
                .map(|name| self.resolve(&name, None))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|template| kind.is_none_or(|kind| template.manifest.kind == kind))
                .map(|template| GenerationTemplateSummary {
                    name: template.manifest.name,
                    kind: template.manifest.kind,
                    description: template.manifest.description,
                })
                .collect::<Vec<_>>();
            templates.sort_by(|left, right| left.name.cmp(&right.name));
            Ok(templates)
        })())
    }

    fn load(&self, name: &str, kind: TemplateKind) -> SfumatoResult<GenerationTemplate> {
        template_result(self.resolve(name, Some(kind)))
    }

    fn create(
        &self,
        name: &str,
        kind: TemplateKind,
        source_path: Option<PathBuf>,
    ) -> SfumatoResult<GenerationTemplate> {
        template_result((|| {
            validate_template_name(name)?;
            let root = self.root.join(name);
            if root.exists() {
                bail!("Generation template '{name}' already exists");
            }
            let source = match source_path {
                Some(path) => fs::read_to_string(&path).with_context(|| {
                    format!("Could not read template source {}", path.display())
                })?,
                None => scaffold(kind),
            };
            validate_template_source(&source)?;
            fs::create_dir_all(&root)?;
            let source_name = kind.source_filename();
            fs::write(root.join(source_name), source)?;
            write_toml(
                &root.join("template.toml"),
                &GenerationTemplateManifest {
                    schema_version: TEMPLATE_SCHEMA_VERSION,
                    name: name.to_string(),
                    kind,
                    description: format!("Reusable {kind} structure: {name}"),
                    source: PathBuf::from(source_name),
                },
            )?;
            self.resolve(name, Some(kind))
        })())
    }
}

fn validate_manifest(
    root: &Path,
    requested_name: &str,
    expected_kind: Option<TemplateKind>,
    manifest: &GenerationTemplateManifest,
) -> Result<()> {
    if manifest.schema_version != TEMPLATE_SCHEMA_VERSION {
        bail!(
            "Generation template '{}' uses unsupported schema version {}",
            manifest.name,
            manifest.schema_version
        );
    }
    if manifest.name != requested_name {
        bail!(
            "Template directory '{requested_name}' does not match manifest name '{}'",
            manifest.name
        );
    }
    if expected_kind.is_some_and(|kind| manifest.kind != kind) {
        bail!(
            "Generation template '{requested_name}' is for {}, not {}",
            manifest.kind,
            expected_kind.expect("checked")
        );
    }
    if manifest.source.is_absolute()
        || manifest
            .source
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "Template source path '{}' must stay inside its package",
            manifest.source.display()
        );
    }
    if !root.join(&manifest.source).is_file() {
        bail!(
            "Template source '{}' does not exist",
            root.join(&manifest.source).display()
        );
    }
    Ok(())
}

fn scaffold(kind: TemplateKind) -> String {
    match kind {
        TemplateKind::Slides => {
            format!("---\nmarp: true\npaginate: true\n---\n\n{TEMPLATE_CONTENT_SLOT}\n")
        }
        TemplateKind::Page => format!(
            "<main id=\"sfumato-template\" class=\"sfumato-template\">\n  {TEMPLATE_CONTENT_SLOT}\n</main>\n"
        ),
    }
}

fn template_result<T>(result: Result<T>) -> SfumatoResult<T> {
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

#[cfg(test)]
#[path = "../tests/unit/templates.rs"]
mod tests;
