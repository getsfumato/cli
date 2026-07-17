#![warn(missing_docs)]
//! Application use cases and outbound ports for Sfumato.
//!
//! Frontends depend on [`application::SfumatoApplication`]. Concrete storage,
//! provider, prompt, filesystem, and renderer implementations live in
//! `sfumato-adapters`.

pub mod application;
pub mod artifacts;
/// Configuration aggregates and effective model/theme resolution.
#[allow(missing_docs)]
pub mod config;
/// Structured configuration editing port.
#[allow(missing_docs)]
pub mod config_editor;
/// Connector configuration services.
#[allow(missing_docs)]
pub mod connectors;
pub mod errors;
pub mod filesystem;
/// Provider-neutral generation request and result DTOs.
#[allow(missing_docs)]
pub mod generation;
/// Named model profile and default-management services.
#[allow(missing_docs)]
pub mod models;
pub mod operation;
/// Offline JavaScript plugin packages available to generated pages.
pub mod page_plugins;
pub mod project_assets;
/// Registered-project application services.
#[allow(missing_docs)]
pub mod projects;
pub mod prompts;
/// Text and image model ports plus the provider-neutral agent runner.
#[allow(missing_docs)]
pub mod providers;
pub mod renderers;
pub mod repositories;
/// Resource-specific application workflows.
#[allow(missing_docs)]
pub mod resources;
pub mod review;
pub mod secrets;
/// User initialization use case.
#[allow(missing_docs)]
pub mod setup;
pub mod sources;
pub mod templates;
/// Theme package entities and application services.
#[allow(missing_docs)]
pub mod themes;
pub mod tools;
