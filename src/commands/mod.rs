use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::Value;

use crate::{
    cli::{
        Commands, ConfigCommands, ConfigDeleteArgs, ConfigSetArgs, ConfigShowArgs,
        ConnectorCommands, ConnectorSetupArgs, ConnectorShowArgs, EditCommands, EditSlidesArgs,
        GenerateCommands, InitProjectArgs, InitTarget, ModelAddArgs, ModelCommands, ModelEditArgs,
        ModelNameArgs, ModelUseArgs, PageArgs, PluginCommands, ProjectCommands, ProjectNameArgs,
        ProjectShowArgs, PromptCommands, PromptCustomizeArgs, PromptProjectArgs, PromptScope,
        PromptShowArgs, SlidesArgs, ThemeCommands, ThemeUseArgs,
    },
    init::InitService,
};
use sfumato_core::{
    application::{
        EditSlidesCommand, GeneratePageCommand, GenerateSlidesCommand, SfumatoApplication,
    },
    config::{Capability, ConfigOverrides},
    config_editor::ConfigTarget,
    connectors::ConnectorPreset as CoreConnectorPreset,
    generation::{GenerationRequest, ResourceKind},
    operation::OperationContext,
    prompts::{PromptId, PromptOrigin, PromptOverrideScope},
    providers::TextGenerationEvent,
    resources::pages::GeneratePageResult,
    resources::slides::{EditSlidesRequest, EditSlidesResult, GenerateSlidesResult},
};

#[async_trait]
pub trait RunnableCommand {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()>;
}

#[async_trait]
impl RunnableCommand for Commands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Init { target } => target.run(application).await,
            Self::Config { command } => command.run(application).await,
            Self::Project { command } => command.run(application).await,
            Self::Connector { command } => command.run(application).await,
            Self::Model { command } => command.run(application).await,
            Self::Theme { command } => command.run(application).await,
            Self::Prompt { command } => command.run(application).await,
            Self::Plugin { command } => command.run(application).await,
            Self::Generate { command } => command.run(application).await,
            Self::Edit { command } => command.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for PluginCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::List => {
                for plugin in application.list_page_plugins()? {
                    println!(
                        "{}\t{}\t{}\t{}",
                        plugin.id, plugin.name, plugin.version, plugin.runtime_hash
                    );
                }
                Ok(())
            }
            Self::Show(args) => {
                let plugin = application.show_page_plugin(&args.id)?;
                println!(
                    "{} {}\nID: {}\nAPI: {}\nSHA-256: {}\nLicense: {}\n\n{}",
                    plugin.summary.name,
                    plugin.summary.version,
                    plugin.summary.id,
                    plugin.summary.api_global,
                    plugin.summary.runtime_hash,
                    plugin.summary.license,
                    plugin.guidance,
                );
                Ok(())
            }
        }
    }
}

#[async_trait]
impl RunnableCommand for PromptCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::List(args) => args.list(&application),
            Self::Show(args) => args.run(Arc::clone(&application)).await,
            Self::Customize(args) => args.run(application).await,
            Self::Validate(args) => args.validate(&application),
        }
    }
}

impl PromptProjectArgs {
    fn list(self, application: &SfumatoApplication) -> Result<()> {
        for prompt in application.list_prompts(self.project)? {
            println!(
                "{}\t{}",
                prompt.id,
                prompt_origin_label(&prompt.provenance.origin)
            );
        }
        Ok(())
    }

    fn validate(self, application: &SfumatoApplication) -> Result<()> {
        let resolved = application.validate_prompts(self.project)?;
        println!("Validated {} prompt templates.", resolved.len());
        for prompt in resolved {
            println!(
                "{}\t{}\t{}",
                prompt.id,
                prompt_origin_label(&prompt.origin),
                &prompt.content_hash[..12]
            );
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for PromptShowArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let id = PromptId::from_str(&self.id)?;
        let source = application.show_prompt(id, self.project)?;
        println!(
            "# {}\n# origin: {}\n# sha256: {}\n\n{}",
            id,
            prompt_origin_label(&source.provenance.origin),
            source.provenance.content_hash,
            source.text
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for PromptCustomizeArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let id = PromptId::from_str(&self.id)?;
        let scope = match self.scope {
            PromptScope::User => PromptOverrideScope::User,
            PromptScope::Project => PromptOverrideScope::Project,
        };
        let path = application.customize_prompt(id, scope, self.project)?;
        println!("Created prompt override at {}", path.display());
        Ok(())
    }
}

fn prompt_origin_label(origin: &PromptOrigin) -> String {
    match origin {
        PromptOrigin::Bundled => "bundled".to_string(),
        PromptOrigin::User(path) => format!("user:{}", path.display()),
        PromptOrigin::Project(path) => format!("project:{}", path.display()),
    }
}

#[async_trait]
impl RunnableCommand for ModelCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Add(args) => args.run(Arc::clone(&application)).await,
            Self::Edit(args) => args.run(Arc::clone(&application)).await,
            Self::List => {
                let models = application.list_models()?;
                if models.is_empty() {
                    println!("No registered model profiles.");
                }
                for model in models {
                    let capabilities = model
                        .capabilities
                        .iter()
                        .map(|capability| capability.as_str())
                        .collect::<Vec<_>>()
                        .join(",");
                    println!(
                        "{}\t{}\t{}\t{capabilities}",
                        model.name, model.connector, model.model
                    );
                }
                Ok(())
            }
            Self::Show(args) => args.run(Arc::clone(&application)).await,
            Self::Remove(args) => {
                let name = application.remove_model(&args.name)?;
                println!("Removed model profile '{name}'");
                Ok(())
            }
            Self::Use(args) => args.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ModelEditArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        application.edit_model(
            &self.name,
            self.connector,
            self.model_id,
            self.capabilities,
            self.options,
        )?;
        println!("Updated model profile '{}'", self.name);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ModelAddArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let name = self.name.clone();
        application.add_model(
            self.name,
            self.connector,
            self.model_id,
            self.capabilities,
            self.options,
        )?;
        println!("Added model profile '{name}'");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ModelNameArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        println!(
            "{}",
            toml::to_string_pretty(&application.show_model(&self.name)?)?
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ModelUseArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let changed =
            application.use_model(&self.selector, &self.profile, self.project.as_deref())?;
        if let Some(project) = changed.project {
            println!(
                "Project '{project}' now uses model profile '{}' for '{}'",
                changed.profile,
                changed.selection.as_str()
            );
        } else {
            println!(
                "User default for '{}' is now model profile '{}'",
                changed.selection.as_str(),
                changed.profile
            );
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ThemeCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Create(args) => {
                let theme = application.create_theme(&args.name)?;
                println!(
                    "Created theme '{}' at {}",
                    theme.manifest.name,
                    theme.root.display()
                );
                Ok(())
            }
            Self::List => {
                for theme in application.list_themes()? {
                    println!("{}", theme.name);
                }
                Ok(())
            }
            Self::Show(args) => {
                let theme = application.show_theme(&args.name)?;
                println!("{}", toml::to_string_pretty(&theme.manifest)?);
                Ok(())
            }
            Self::Use(args) => args.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ThemeUseArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let project = application.use_theme(&self.name, self.project.as_deref())?;
        println!(
            "Project '{}' now uses theme '{}'",
            project.name, project.theme
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConnectorCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::List => {
                for connector in application.list_connectors()? {
                    println!("{}\t{}", connector.name, connector.base_url);
                }
                Ok(())
            }
            Self::Show(args) => args.run(Arc::clone(&application)).await,
            Self::Setup(args) => args.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ConnectorShowArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        println!(
            "{}",
            toml::to_string_pretty(&application.show_connector(&self.name)?)?
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConnectorSetupArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let preset = match self.preset {
            crate::cli::ConnectorPreset::Ollama => CoreConnectorPreset::Ollama,
            crate::cli::ConnectorPreset::Openrouter => CoreConnectorPreset::Openrouter,
        };
        let connector = application.setup_connector(preset, self.name, self.api_key_env)?;
        println!(
            "Configured OpenAI-compatible connector '{}'",
            connector.name
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for InitTarget {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::User { yes, force } => {
                InitService::new(application).write_user_config(yes, force)
            }
            Self::Project(args) => args.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for InitProjectArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let project = application.init_project(self.name, self.path, !self.no_activate)?;
        println!("Initialized project '{}'", project.name);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ProjectCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::List => {
                let projects = application.list_projects()?;
                if projects.is_empty() {
                    println!("No registered projects.");
                }
                for project in projects {
                    let marker = if project.active { "*" } else { " " };
                    println!("{marker} {}\t{}", project.name, project.path.display());
                }
                Ok(())
            }
            Self::Show(args) => args.run(Arc::clone(&application)).await,
            Self::Use(args) => args.run(application).await,
            Self::Remove(args) => {
                let removed = application.remove_project(&args.name)?;
                println!(
                    "Removed project '{}' from the registry; project files were kept.",
                    removed.name
                );
                Ok(())
            }
        }
    }
}

#[async_trait]
impl RunnableCommand for ProjectShowArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        println!(
            "{}",
            toml::to_string_pretty(&application.show_project(self.name.as_deref())?)?
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ProjectNameArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let name = application.use_project(&self.name)?;
        println!("Active project: {name}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Show(args) => args.run(Arc::clone(&application)).await,
            Self::Set(args) => args.run(Arc::clone(&application)).await,
            Self::Delete(args) => args.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ConfigShowArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let rendered =
            application.show_config(config_target(self.scope), self.project, self.key)?;
        println!("{rendered}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigSetArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let path = application.set_config(
            config_target(self.scope),
            self.project,
            &self.key,
            &self.value,
        )?;
        println!("Set {} in {}", self.key, path.display());
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigDeleteArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let path = application.delete_config(config_target(self.scope), self.project, &self.key)?;
        println!("Deleted {} from {}", self.key, path.display());
        Ok(())
    }
}

fn config_target(scope: crate::cli::ConfigScope) -> ConfigTarget {
    match scope {
        crate::cli::ConfigScope::User => ConfigTarget::User,
        crate::cli::ConfigScope::Project => ConfigTarget::Project,
        crate::cli::ConfigScope::Effective => ConfigTarget::Effective,
    }
}

#[async_trait]
impl RunnableCommand for GenerateCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Slides(args) => args.run(application).await,
            Self::Page(args) => args.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for PageArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let json = self.json;
        let dry_run = self.dry_run;
        let event_sink = (!json && !dry_run)
            .then_some(std::sync::Arc::new(render_generation_event)
                as std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>);
        match execute_page(&application, self, event_sink, OperationContext::detached()).await {
            Ok(result) => render_page_result(result, json, dry_run),
            Err(error) if json => {
                println!("{}", json_operation_error(&error));
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

pub(crate) async fn execute_page(
    application: &SfumatoApplication,
    args: PageArgs,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: OperationContext,
) -> Result<GeneratePageResult> {
    if args.instruction.trim().is_empty() {
        bail!("Instruction cannot be empty");
    }
    let model_overrides = parse_model_overrides(&args.model_overrides)?;
    let config = ConfigOverrides {
        project: args.project.clone(),
        theme: args.theme,
        model_overrides: model_overrides.clone(),
        reviewer_model: args.review_model,
        publish_dir: args.out,
        pdf: false,
    };
    let request = GenerationRequest {
        instruction: args.instruction,
        sources: args.inputs,
        resource_kind: ResourceKind::Page,
        project: args.project,
        model_overrides,
    };
    Ok(application
        .generate_page(GeneratePageCommand {
            operation,
            config,
            request,
            title: args.title,
            plugins: args.plugins,
            dry_run: args.dry_run,
            review: !args.no_review,
            event_sink,
        })
        .await?)
}

fn render_page_result(result: GeneratePageResult, json: bool, dry_run: bool) -> Result<()> {
    for warning in &result.warnings {
        eprintln!("{warning}");
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&result.output)?);
    } else if dry_run {
        println!("Project: {}", result.output.project);
        println!("Planned HTML: {}", result.html_path.display());
        if result.output.plugins.is_empty() {
            println!("Plugins: none");
        } else {
            println!("Plugins:");
            for plugin in &result.output.plugins {
                println!("- {} {} ({})", plugin.name, plugin.version, plugin.id);
            }
        }
        if !result.tool_summaries.is_empty() {
            println!("Injected tools:");
            for tool in &result.tool_summaries {
                println!("- {}: {}", tool.name, tool.description);
            }
        }
        if let Some(prompt) = &result.prompt_preview {
            println!("\n{prompt}");
        }
        println!("Dry run complete; no model or browser was called and no files were written.");
    } else {
        println!("Wrote {}", result.html_path.display());
        for path in &result.published_paths {
            println!("Published {}", path.display());
        }
    }
    Ok(())
}

#[async_trait]
impl RunnableCommand for EditCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Slides(args) => args.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for EditSlidesArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let json = self.json;
        let event_sink = (!json).then_some(std::sync::Arc::new(render_generation_event)
            as std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>);
        match execute_edit_slides(&application, self, event_sink, OperationContext::detached())
            .await
        {
            Ok(result) => render_edit_slides_result(result, json),
            Err(error) if json => {
                println!("{}", json_operation_error(&error));
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

pub(crate) async fn execute_edit_slides(
    application: &SfumatoApplication,
    args: EditSlidesArgs,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: OperationContext,
) -> Result<EditSlidesResult> {
    if args.instruction.trim().is_empty() {
        bail!("Instruction cannot be empty");
    }
    let model_overrides = parse_model_overrides(&args.model_overrides)?;
    if model_overrides
        .keys()
        .any(|capability| *capability != Capability::Text)
    {
        bail!("Slide editing only accepts a text model override");
    }
    let config = ConfigOverrides {
        project: args.project,
        theme: None,
        model_overrides,
        reviewer_model: None,
        publish_dir: None,
        pdf: true,
    };
    Ok(application
        .edit_slides(EditSlidesCommand {
            operation,
            config,
            request: EditSlidesRequest {
                markdown_path: args.markdown_path,
                instruction: args.instruction,
            },
            event_sink,
        })
        .await?)
}

fn render_edit_slides_result(result: EditSlidesResult, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }
    for warning in &result.warnings {
        eprintln!("{warning}");
    }
    if result.operations == 0 {
        println!("No content changes were needed; regenerated the PDF.");
    } else {
        println!(
            "Applied {} patch operation(s) to {} slide(s) with '{}'.",
            result.operations,
            result.changed_slides.len(),
            result.model
        );
    }
    println!("Wrote {}", result.markdown_path.display());
    println!("Wrote {}", result.pdf_path.display());
    Ok(())
}

#[async_trait]
impl RunnableCommand for SlidesArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let json = self.json;
        let dry_run = self.dry_run;
        let event_sink = (!json && !dry_run)
            .then_some(std::sync::Arc::new(render_generation_event)
                as std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>);
        match execute_slides(&application, self, event_sink, OperationContext::detached()).await {
            Ok(result) => {
                render_slides_result(result, json, dry_run)?;
                Ok(())
            }
            Err(error) if json => {
                println!("{}", json_operation_error(&error));
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

fn json_operation_error(error: &anyhow::Error) -> serde_json::Value {
    error
        .downcast_ref::<sfumato_core::errors::SfumatoError>()
        .map(|error| serde_json::json!({ "error": error }))
        .unwrap_or_else(|| serde_json::json!({ "error": { "message": format!("{error:#}") } }))
}

pub(crate) async fn execute_slides(
    application: &SfumatoApplication,
    args: SlidesArgs,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: OperationContext,
) -> Result<GenerateSlidesResult> {
    if args.instruction.trim().is_empty() {
        bail!("Instruction cannot be empty");
    }
    let model_overrides = parse_model_overrides(&args.model_overrides)?;
    let config = ConfigOverrides {
        project: args.project.clone(),
        theme: args.theme,
        model_overrides: model_overrides.clone(),
        reviewer_model: args.review_model,
        publish_dir: args.out,
        pdf: args.pdf,
    };
    let request = GenerationRequest {
        instruction: args.instruction,
        sources: args.inputs,
        resource_kind: ResourceKind::Slides,
        project: args.project,
        model_overrides,
    };
    Ok(application
        .generate_slides(GenerateSlidesCommand {
            operation,
            config,
            request,
            title: args.title,
            dry_run: args.dry_run,
            review: !args.no_review,
            event_sink,
        })
        .await?)
}

fn render_slides_result(result: GenerateSlidesResult, json: bool, dry_run: bool) -> Result<()> {
    for warning in &result.warnings {
        eprintln!("{warning}");
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&result.output)?);
    } else if dry_run {
        match &result.output.project_instructions {
            Some(path) => println!("Project instructions: {}\n", path.display()),
            None => println!("Project instructions: no SFUMATO.md found\n"),
        }
        if !result.tool_summaries.is_empty() {
            println!("Injected tools:");
            for tool in &result.tool_summaries {
                println!("- {}: {}", tool.name, tool.description);
            }
            println!();
        }
        if let Some(prompt) = &result.prompt_preview {
            println!("{prompt}");
        }
        if result.output.review.enabled {
            let reviewer = result
                .output
                .models
                .get("reviewer")
                .map(String::as_str)
                .unwrap_or("draft model");
            println!(
                "Review: enabled with model profile '{reviewer}' (semantic review and conditional layout repair)."
            );
        } else {
            println!("Review: disabled.");
        }
        println!("Dry run complete; no files were written.");
    } else {
        println!("Wrote {}", result.markdown_path.display());
        if let Some(pdf_path) = &result.pdf_path {
            println!("Wrote {}", pdf_path.display());
        }
        if let Some(published_pdf_path) = &result.published_pdf_path {
            println!("Published {}", published_pdf_path.display());
        }
    }
    Ok(())
}

fn render_generation_event(event: TextGenerationEvent) {
    match event {
        TextGenerationEvent::StageStarted { stage, profile } => {
            let profile = profile
                .map(|profile| format!(" with {}", bold(&profile)))
                .unwrap_or_default();
            eprintln!(
                "{} {}{}",
                styled_label("stage", ANSI_YELLOW),
                stage.as_str(),
                profile
            );
        }
        TextGenerationEvent::RequestStarted { round } => {
            eprintln!(
                "{} {}",
                styled_label("model", ANSI_CYAN),
                dim(&format!("request round {round}"))
            );
        }
        TextGenerationEvent::ToolCallRequested { name, arguments } => {
            eprintln!(
                "{} {} {}",
                styled_label("tool call", ANSI_MAGENTA),
                bold(&name),
                format_tool_arguments(&arguments)
            );
        }
        TextGenerationEvent::ToolCallSucceeded { name, result } => {
            eprintln!(
                "{} {} {}",
                styled_label("tool result", ANSI_GREEN),
                bold(&name),
                format_tool_result(&name, &result)
            );
        }
        TextGenerationEvent::ToolCallFailed { name, error } => {
            eprintln!(
                "{} {} {}",
                styled_label("tool error", ANSI_RED),
                bold(&name),
                red(&compact_preview(&error, 180))
            );
        }
        TextGenerationEvent::ResponseCompleted => {
            eprintln!(
                "{} {}",
                styled_label("model", ANSI_CYAN),
                green("response complete")
            );
        }
        TextGenerationEvent::DraftTitleRepairStarted { error } => {
            eprintln!(
                "{} {}",
                styled_label("title repair", ANSI_YELLOW),
                yellow(&compact_preview(&error, 220))
            );
        }
        TextGenerationEvent::ReviewRetryStarted { attempt, error } => {
            eprintln!(
                "{} attempt {attempt}: {}",
                styled_label("review retry", ANSI_YELLOW),
                yellow(&compact_preview(&error, 220))
            );
        }
        TextGenerationEvent::ContextCompactionStarted {
            stage,
            original_chars,
            compacted_chars,
        } => {
            eprintln!(
                "{} {} context from {} to {} characters",
                styled_label("recovery", ANSI_YELLOW),
                stage.as_str(),
                yellow(&original_chars.to_string()),
                green(&compacted_chars.to_string())
            );
        }
        TextGenerationEvent::LayoutCheckCompleted { issues } => {
            let result = if issues == 0 {
                green("no overflow detected")
            } else {
                yellow(&format!("{issues} slide(s) need repair"))
            };
            eprintln!("{} {}", styled_label("layout", ANSI_CYAN), result);
        }
        TextGenerationEvent::LayoutSlideRepairStarted {
            slide,
            position,
            total,
            profile,
        } => {
            eprintln!(
                "{} slide {} ({position}/{total}) with {}",
                styled_label("repair", ANSI_MAGENTA),
                bold(&slide.to_string()),
                bold(&profile)
            );
        }
        TextGenerationEvent::LayoutSlideRepairRetryStarted {
            slide,
            attempt,
            error,
        } => {
            eprintln!(
                "{} slide {} attempt {attempt}: {}",
                styled_label("repair retry", ANSI_YELLOW),
                bold(&slide.to_string()),
                yellow(&compact_preview(&error, 220))
            );
        }
    }
}

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_ITALIC: &str = "\x1b[3m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_MAGENTA: &str = "\x1b[35m";

fn styled_label(label: &str, color: &str) -> String {
    format!("{ANSI_BOLD}{color}{label}:{ANSI_RESET}")
}

fn bold(value: &str) -> String {
    format!("{ANSI_BOLD}{value}{ANSI_RESET}")
}

fn dim(value: &str) -> String {
    format!("{ANSI_DIM}{value}{ANSI_RESET}")
}

fn italic(value: &str) -> String {
    format!("{ANSI_ITALIC}{value}{ANSI_RESET}")
}

fn green(value: &str) -> String {
    format!("{ANSI_GREEN}{value}{ANSI_RESET}")
}

fn red(value: &str) -> String {
    format!("{ANSI_RED}{value}{ANSI_RESET}")
}

fn yellow(value: &str) -> String {
    format!("{ANSI_YELLOW}{value}{ANSI_RESET}")
}

fn format_tool_arguments(arguments: &Value) -> String {
    let arguments = parse_maybe_json_string(arguments);
    if let Some(path) = arguments.get("path").and_then(Value::as_str) {
        return format!("path {}", italic(path));
    }
    if let Some(prompt) = arguments.get("prompt").and_then(Value::as_str) {
        return format!("prompt {}", italic(&compact_preview(prompt, 140)));
    }
    dim(&compact_preview(&arguments.to_string(), 140))
}

fn format_tool_result(name: &str, result: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return dim(&compact_preview(result, 180));
    };

    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return red(&format!("returned error: {}", compact_preview(error, 180)));
    }

    if name == "sfumato_list_directory" || value.get("entries").is_some() {
        return summarize_directory_listing(&value);
    }

    if name == "sfumato_read_file" || value.get("content").is_some() {
        return summarize_file_read(&value);
    }

    if name == "sfumato_image_gen" || value.get("markdown_path").is_some() {
        let path = value
            .get("markdown_path")
            .and_then(Value::as_str)
            .unwrap_or("generated image");
        let profile = value
            .get("model_profile")
            .and_then(Value::as_str)
            .map(|profile| format!(" with {}", bold(profile)))
            .unwrap_or_default();
        return format!("created {}{}", italic(path), profile);
    }

    dim(&compact_preview(result, 180))
}

fn summarize_directory_listing(value: &Value) -> String {
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("unknown path");
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let directories = entries
        .iter()
        .filter(|entry| entry.get("kind").and_then(Value::as_str) == Some("directory"))
        .count();
    let files = entries
        .iter()
        .filter(|entry| entry.get("kind").and_then(Value::as_str) == Some("file"))
        .count();
    let names = entries
        .iter()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .take(6)
        .collect::<Vec<_>>();
    let truncated_by_display = entries.len() > names.len();
    let truncated_by_tool = value
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let suffix = if names.is_empty() {
        String::new()
    } else {
        let more = if truncated_by_display || truncated_by_tool {
            ", ..."
        } else {
            ""
        };
        format!(" — {}", dim(&format!("{}{}", names.join(", "), more)))
    };

    format!(
        "listed {} — {} entries ({} files, {} directories){}",
        italic(path),
        yellow(&entries.len().to_string()),
        files,
        directories,
        suffix
    )
}

fn summarize_file_read(value: &Value) -> String {
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("unknown file");
    let content = value.get("content").and_then(Value::as_str).unwrap_or("");
    let chars = content.chars().count();
    let lines = content.lines().count();
    let preview = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| compact_preview(line, 120));
    let preview = preview
        .map(|line| format!(" — {}", dim(&line)))
        .unwrap_or_default();

    format!(
        "read {} — {} chars, {} lines{}",
        italic(path),
        yellow(&chars.to_string()),
        lines,
        preview
    )
}

fn parse_maybe_json_string(value: &Value) -> Value {
    if let Value::String(raw) = value {
        serde_json::from_str(raw).unwrap_or_else(|_| value.clone())
    } else {
        value.clone()
    }
}

fn compact_preview(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

fn parse_model_overrides(values: &[String]) -> Result<BTreeMap<Capability, String>> {
    values
        .iter()
        .map(|value| {
            let (capability, profile) = value.split_once('=').with_context(|| {
                format!("Invalid model override '{value}'. Use capability=profile.")
            })?;
            if profile.trim().is_empty() {
                bail!("Model profile cannot be empty in '{value}'");
            }
            Ok((
                Capability::from_str(capability.trim())?,
                profile.trim().to_string(),
            ))
        })
        .collect()
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/commands.rs"]
mod tests;
