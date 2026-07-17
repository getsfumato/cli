//! Offline JavaScript plugin catalog contracts for generated pages.

use serde::Serialize;

use crate::errors::SfumatoResult;

/// Public metadata for one bundled page plugin.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PagePluginSummary {
    /// Stable command-line identifier.
    pub id: String,
    /// Human-readable package name.
    pub name: String,
    /// Exact bundled package version.
    pub version: String,
    /// Browser global exposed to generated JavaScript.
    pub api_global: String,
    /// SHA-256 digest of the bundled runtime.
    pub runtime_hash: String,
    /// Relative bundled license asset.
    pub license: String,
}

/// Resolved plugin metadata, model guidance, and offline browser runtime.
#[derive(Clone, Debug)]
pub struct PagePluginPackage {
    /// Public package metadata.
    pub summary: PagePluginSummary,
    /// Model-facing usage guide.
    pub guidance: String,
    /// Offline JavaScript runtime injected into generated pages.
    pub runtime_javascript: String,
}

/// Resolves deterministic bundled page plugins without frontend knowledge.
pub trait PagePluginCatalog: Send + Sync {
    /// Lists bundled plugins in stable identifier order.
    fn list(&self) -> SfumatoResult<Vec<PagePluginSummary>>;
    /// Resolves one plugin package by stable identifier.
    fn load(&self, id: &str) -> SfumatoResult<PagePluginPackage>;

    /// Resolves, deduplicates, and deterministically orders selected plugins.
    fn resolve(&self, ids: &[String]) -> SfumatoResult<Vec<PagePluginPackage>> {
        let mut ids = ids.to_vec();
        ids.sort();
        ids.dedup();
        ids.iter().map(|id| self.load(id)).collect()
    }
}
