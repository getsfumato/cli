//! Process-backed resource renderer adapters.

mod diagrams;
mod document;
mod marp;

pub use diagrams::MermaidCliRenderer;
pub use document::PagedDocumentCliRenderer;
pub use marp::MarpCliRenderer;
