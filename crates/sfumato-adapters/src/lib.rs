#![warn(missing_docs)]
//! Infrastructure adapters for Sfumato application ports.

pub mod anthropic;
pub mod application;
pub mod artifacts;
pub mod codex_app_server;
mod config_dto;
pub mod config_editor;
pub mod config_files;
pub mod documents;
pub mod elevenlabs;
pub mod filesystem;
pub mod lmstudio;
pub mod ollama;
pub(crate) mod openai_compatible;
pub mod openrouter;
pub mod page_plugins;
pub mod pages;
pub mod project_assets;
pub mod prompts;
pub mod providers;
pub mod renderers;
pub mod repositories;
mod runtime;
pub mod secrets;
pub mod sources;
pub mod templates;
pub mod themes;
pub mod tools;
pub mod videos;
