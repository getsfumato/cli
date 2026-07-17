#![warn(missing_docs)]
//! Infrastructure adapters for Sfumato application ports.

pub mod application;
pub mod artifacts;
mod config_dto;
pub mod config_editor;
pub mod config_files;
pub mod filesystem;
pub(crate) mod openai_compatible;
pub mod page_plugins;
pub mod pages;
pub mod project_assets;
pub mod prompts;
pub mod renderers;
pub mod repositories;
mod runtime;
pub mod sources;
pub mod templates;
pub mod themes;
pub mod tools;
