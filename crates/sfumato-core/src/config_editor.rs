//! Generic configuration editing contract.

use std::path::PathBuf;

use crate::errors::SfumatoResult;

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
    ) -> SfumatoResult<String>;

    fn set(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: &str,
        raw_value: &str,
    ) -> SfumatoResult<PathBuf>;

    fn delete(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: &str,
    ) -> SfumatoResult<PathBuf>;
}
