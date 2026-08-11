//! Generic configuration editing contract.

use std::path::PathBuf;

use crate::errors::SfumatoResult;

/// Which configuration document an edit or a read addresses.
#[derive(Clone, Copy, Debug)]
pub enum ConfigTarget {
    /// The user-global document, shared by every project.
    User,
    /// One project's own document, which overrides the user-global one.
    Project,
    /// The merge of both, plus per-run overrides. Read-only: it is a computed
    /// view and has no file to write to.
    Effective,
}

/// Schema-aware configuration inspection and focused edits.
///
/// Every write validates the *whole* resulting document before it lands, so a
/// single-key edit cannot leave configuration in a state the rest of the system
/// would reject. Secrets are never returned by [`Self::show`].
pub trait ConfigEditor: Send + Sync {
    /// Renders a scope, or one dotted key within it, as TOML.
    ///
    /// `project` selects which project's document to read; `None` uses the active
    /// one. `key` is a dotted path such as `defaults.text`; `None` returns the whole
    /// scope. Secret values are redacted rather than resolved.
    fn show(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: Option<String>,
    ) -> SfumatoResult<String>;

    /// Sets one dotted key, returning the path of the document written.
    ///
    /// `raw_value` is parsed as TOML first and treated as a plain string only if
    /// that fails, so arrays and tables can be set as well as scalars. Rejects
    /// [`ConfigTarget::Effective`], which has no file, and refuses raw credentials.
    fn set(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: &str,
        raw_value: &str,
    ) -> SfumatoResult<PathBuf>;

    /// Removes one dotted key, returning the path of the document written.
    ///
    /// Succeeds only when the document remains valid without it, so a required key
    /// cannot be deleted into an invalid state.
    fn delete(
        &self,
        scope: ConfigTarget,
        project: Option<String>,
        key: &str,
    ) -> SfumatoResult<PathBuf>;
}
