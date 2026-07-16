//! Process-backed resource renderer adapters.

mod diagrams;
mod marp;

pub use diagrams::MermaidCliRenderer;
pub use marp::MarpCliRenderer;
