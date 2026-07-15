use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};

pub const PROJECT_INSTRUCTIONS_FILE: &str = "SFUMATO.md";
const MAX_PROJECT_INSTRUCTIONS_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectInstructions {
    pub path: PathBuf,
    pub content: String,
}

impl ProjectInstructions {
    pub fn load(project_root: &std::path::Path) -> Result<Option<Self>> {
        let path = project_root.join(PROJECT_INSTRUCTIONS_FILE);
        if !path
            .try_exists()
            .with_context(|| format!("Could not inspect {}", path.display()))?
        {
            return Ok(None);
        }

        let canonical_root = project_root.canonicalize().with_context(|| {
            format!("Could not resolve project root {}", project_root.display())
        })?;
        let canonical_path = path
            .canonicalize()
            .with_context(|| format!("Could not resolve {}", path.display()))?;
        if !canonical_path.starts_with(&canonical_root) {
            bail!(
                "Project instructions {} resolve outside project root {}",
                path.display(),
                project_root.display()
            );
        }

        let metadata = fs::metadata(&canonical_path)
            .with_context(|| format!("Could not inspect {}", path.display()))?;
        if !metadata.is_file() {
            bail!("Project instructions path {} is not a file", path.display());
        }
        if metadata.len() > MAX_PROJECT_INSTRUCTIONS_BYTES {
            bail!(
                "Project instructions {} are {} bytes; the maximum is {} bytes",
                path.display(),
                metadata.len(),
                MAX_PROJECT_INSTRUCTIONS_BYTES
            );
        }

        let content = fs::read_to_string(&canonical_path)
            .with_context(|| format!("Could not read project instructions {}", path.display()))?;
        Ok(Some(Self {
            path,
            content: content.trim().to_string(),
        }))
    }

    pub fn prompt_section(&self) -> String {
        format!(
            "Project instructions loaded from {}:\n<sfumato_project_instructions>\n{}\n</sfumato_project_instructions>",
            self.path.display(),
            self.content
        )
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/instructions.rs"]
mod tests;
