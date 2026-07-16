//! Generic configuration editing contract.

use std::path::PathBuf;

use anyhow::Result;

/// User-visible configuration scope.
#[derive(Clone, Copy, Debug)]
pub enum ConfigTarget {
    User,
    Project,
    Effective,
}

/// Port for schema-aware configuration inspection and focused edits.
pub trait ConfigEditor: Send + Sync {
    fn show(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: Option<String>,
    ) -> Result<String>;

    fn set(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: &str,
        raw_value: &str,
    ) -> Result<PathBuf>;

    fn delete(&self, scope: ConfigTarget, project: Option<String>, key: &str) -> Result<PathBuf>;
}
