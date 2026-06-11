use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;

use crate::config::Capability;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct GenerationRequest {
    pub instruction: String,
    pub sources: Vec<PathBuf>,
    pub resource_kind: ResourceKind,
    pub project: Option<String>,
    pub model_overrides: BTreeMap<Capability, String>,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum ResourceKind {
    Slides,
    Html,
    Image,
    Video,
    Audio,
}

#[derive(Debug, Serialize)]
pub struct GenerationOutput {
    pub project: String,
    pub models: BTreeMap<String, String>,
    pub artifacts: Vec<PathBuf>,
}
