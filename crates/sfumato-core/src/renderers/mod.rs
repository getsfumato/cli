//! Renderer ports used by resource workflows.

use std::{collections::BTreeMap, path::Path};

use async_trait::async_trait;
use serde::Serialize;

use crate::{
    errors::{OperationStage, SfumatoResult},
    generation::SlideLayoutIssue,
    operation::OperationContext,
};

/// Semantic Mermaid styling derived from a Sfumato theme.
#[derive(Clone, Debug, Serialize)]
pub struct MermaidThemeConfig {
    theme: &'static str,
    #[serde(rename = "themeVariables")]
    theme_variables: BTreeMap<String, String>,
}

impl MermaidThemeConfig {
    /// Creates a custom Mermaid base-theme configuration.
    pub fn new(theme_variables: BTreeMap<String, String>) -> Self {
        Self {
            theme: "base",
            theme_variables,
        }
    }
}

/// Port for rendering Mermaid source into SVG artifacts.
#[async_trait]
pub trait DiagramRenderer: Send + Sync {
    /// Renders one Mermaid input file into an SVG output file.
    async fn render_svg(
        &self,
        input_path: &Path,
        output_path: &Path,
        theme: &MermaidThemeConfig,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String>;
}

/// Port for Marp PDF rendering and browser-backed layout inspection.
#[async_trait]
pub trait SlideRenderer: Send + Sync {
    /// Renders a themed Marp Markdown file into PDF.
    async fn render_pdf(
        &self,
        markdown_path: &Path,
        theme_css_path: &Path,
        pdf_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> SfumatoResult<()>;

    /// Measures horizontal and vertical overflow in a themed Marp deck.
    async fn inspect_layout(
        &self,
        markdown_path: &Path,
        theme_css_path: &Path,
        html_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<SlideLayoutIssue>>;
}
