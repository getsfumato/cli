//! Offline JavaScript plugin catalog contracts for generated pages.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

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
    /// Broad catalog category such as utility, animation, or ui.
    pub category: PagePluginCategory,
    /// Runtime dependencies resolved automatically before this plugin.
    pub dependencies: Vec<String>,
}

/// Broad model-facing role of a bundled page plugin.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PagePluginCategory {
    /// General visualization or animation helper.
    Utility,
    /// React component library or its required runtime.
    Ui,
    /// Internal runtime dependency normally selected transitively.
    Runtime,
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
        let mut resolved = Vec::new();
        let mut complete = BTreeSet::new();
        let mut visiting = BTreeSet::new();
        for id in ids {
            self.resolve_dependency(&id, &mut complete, &mut visiting, &mut resolved)?;
        }
        Ok(resolved)
    }

    /// Resolves one plugin and its dependency graph in load order.
    fn resolve_dependency(
        &self,
        id: &str,
        complete: &mut BTreeSet<String>,
        visiting: &mut BTreeSet<String>,
        resolved: &mut Vec<PagePluginPackage>,
    ) -> SfumatoResult<()> {
        if complete.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.to_string()) {
            return Err(crate::errors::SfumatoError::validation(format!(
                "Page plugin dependency cycle contains '{id}'"
            )));
        }
        let package = self.load(id)?;
        for dependency in &package.summary.dependencies {
            self.resolve_dependency(dependency, complete, visiting, resolved)?;
        }
        visiting.remove(id);
        complete.insert(id.to_string());
        resolved.push(package);
        Ok(())
    }
}
