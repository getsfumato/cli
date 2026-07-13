pub mod decks;

use anyhow::{Context, Result};
use json_patch::Patch;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFormat {
    SlideDeck,
    Html,
    Python,
    PlainText,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewSnapshot {
    pub schema_version: u32,
    pub format: ReviewFormat,
    pub revision: String,
    pub document: Value,
    pub constraints: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchReport {
    pub operations: usize,
    pub changed_nodes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub warnings: Vec<String>,
}

pub trait ReviewableDocument {
    fn snapshot(&self) -> Result<ReviewSnapshot>;
    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport>;
    fn validate(&self) -> Result<ValidationReport>;
    fn render(&self) -> Result<String>;
}

pub fn parse_json_patch(response: &str) -> Result<Patch> {
    let response = strip_json_fence(response.trim());
    serde_json::from_str(response).context(
        "Reviewer response must be an RFC 6902 JSON Patch array, for example [{\"op\":\"test\",...}]",
    )
}

fn strip_json_fence(value: &str) -> &str {
    let value = value
        .strip_prefix("```json")
        .or_else(|| value.strip_prefix("```JSON"))
        .or_else(|| value.strip_prefix("```"))
        .unwrap_or(value);
    value.strip_suffix("```").unwrap_or(value).trim()
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/review.rs"]
mod tests;
