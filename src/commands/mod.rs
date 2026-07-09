use std::{collections::BTreeMap, str::FromStr};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::Value;

use crate::{
    cli::{
        Commands, ConfigCommands, ConfigDeleteArgs, ConfigSetArgs, ConfigShowArgs,
        ConnectorCommands, ConnectorSetupArgs, ConnectorShowArgs, GenerateCommands,
        InitProjectArgs, InitTarget, ModelAddArgs, ModelCommands, ModelEditArgs, ModelNameArgs,
        ModelUseArgs, ProjectCommands, ProjectNameArgs, ProjectShowArgs, SlidesArgs, ThemeCommands,
        ThemeUseArgs,
    },
    init::InitService,
};
use sfumato_core::{
    config::{Capability, ConfigOverrides, EffectiveConfig},
    config_editor::{ConfigService, ConfigTarget},
    connectors::{ConnectorPreset as CoreConnectorPreset, ConnectorService},
    generation::{GenerationRequest, ResourceKind},
    models::ModelService,
    projects::ProjectService,
    providers::TextGenerationEvent,
    resources::slides::{GenerateSlidesOptions, generate_slides},
    themes::ThemeService,
};

#[async_trait]
pub trait RunnableCommand {
    async fn run(self) -> Result<()>;
}

#[async_trait]
impl RunnableCommand for Commands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Init { target } => target.run().await,
            Self::Config { command } => command.run().await,
            Self::Project { command } => command.run().await,
            Self::Connector { command } => command.run().await,
            Self::Model { command } => command.run().await,
            Self::Theme { command } => command.run().await,
            Self::Generate { command } => command.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ModelCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Add(args) => args.run().await,
            Self::Edit(args) => args.run().await,
            Self::List => {
                let models = ModelService::load()?.list();
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
            Self::Show(args) => args.run().await,
            Self::Remove(args) => {
                let mut service = ModelService::load()?;
                let name = service.remove(&args.name)?;
                println!("Removed model profile '{name}'");
                Ok(())
            }
            Self::Use(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ModelEditArgs {
    async fn run(self) -> Result<()> {
        ModelService::load()?.edit(
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
    async fn run(self) -> Result<()> {
        let name = self.name.clone();
        ModelService::load()?.add(
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
    async fn run(self) -> Result<()> {
        println!("{}", ModelService::load()?.show(&self.name)?);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ModelUseArgs {
    async fn run(self) -> Result<()> {
        let changed = ModelService::load()?.use_default(
            &self.capability,
            &self.profile,
            self.project.as_deref(),
        )?;
        if let Some(project) = changed.project {
            println!(
                "Project '{project}' now uses model profile '{}' for '{}'",
                changed.profile,
                changed.capability.as_str()
            );
        } else {
            println!(
                "User default for '{}' is now model profile '{}'",
                changed.capability.as_str(),
                changed.profile
            );
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ThemeCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Create(args) => {
                let theme = ThemeService::load()?.create(&args.name)?;
                println!(
                    "Created theme '{}' at {}",
                    theme.manifest.name,
                    theme.root.display()
                );
                Ok(())
            }
            Self::List => {
                for theme in ThemeService::load()?.list()? {
                    println!("{}", theme.name);
                }
                Ok(())
            }
            Self::Show(args) => {
                println!("{}", ThemeService::load()?.show(&args.name)?);
                Ok(())
            }
            Self::Use(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ThemeUseArgs {
    async fn run(self) -> Result<()> {
        let project = ThemeService::load()?.use_for_project(&self.name, self.project.as_deref())?;
        println!(
            "Project '{}' now uses theme '{}'",
            project.name, project.theme
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConnectorCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::List => {
                for connector in ConnectorService::load()?.list() {
                    println!("{}\t{}", connector.name, connector.base_url);
                }
                Ok(())
            }
            Self::Show(args) => args.run().await,
            Self::Setup(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ConnectorShowArgs {
    async fn run(self) -> Result<()> {
        println!("{}", ConnectorService::load()?.show(&self.name)?);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConnectorSetupArgs {
    async fn run(self) -> Result<()> {
        let preset = match self.preset {
            crate::cli::ConnectorPreset::Ollama => CoreConnectorPreset::Ollama,
            crate::cli::ConnectorPreset::Openrouter => CoreConnectorPreset::Openrouter,
        };
        let connector = ConnectorService::load()?.setup(preset, self.name, self.api_key_env)?;
        println!(
            "Configured OpenAI-compatible connector '{}'",
            connector.name
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for InitTarget {
    async fn run(self) -> Result<()> {
        match self {
            Self::User { yes, force } => InitService::new()?.write_user_config(yes, force),
            Self::Project(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for InitProjectArgs {
    async fn run(self) -> Result<()> {
        let project = ProjectService::load()?.init(self.name, self.path, !self.no_activate)?;
        println!("Initialized project '{}'", project.name);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ProjectCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::List => {
                let projects = ProjectService::load()?.list()?;
                if projects.is_empty() {
                    println!("No registered projects.");
                }
                for project in projects {
                    let marker = if project.active { "*" } else { " " };
                    println!("{marker} {}\t{}", project.name, project.path.display());
                }
                Ok(())
            }
            Self::Show(args) => args.run().await,
            Self::Use(args) => args.run().await,
            Self::Remove(args) => {
                let service = ProjectService::load()?;
                let removed = service.remove(&args.name)?;
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
    async fn run(self) -> Result<()> {
        println!("{}", ProjectService::load()?.show(self.name.as_deref())?);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ProjectNameArgs {
    async fn run(self) -> Result<()> {
        let name = ProjectService::load()?.use_project(&self.name)?;
        println!("Active project: {name}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Show(args) => args.run().await,
            Self::Set(args) => args.run().await,
            Self::Delete(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ConfigShowArgs {
    async fn run(self) -> Result<()> {
        let rendered =
            ConfigService::new()?.show(config_target(self.scope), self.project, self.key)?;
        println!("{rendered}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigSetArgs {
    async fn run(self) -> Result<()> {
        let path = ConfigService::new()?.set(
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
    async fn run(self) -> Result<()> {
        let path =
            ConfigService::new()?.delete(config_target(self.scope), self.project, &self.key)?;
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
    async fn run(self) -> Result<()> {
        match self {
            Self::Slides(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for SlidesArgs {
    async fn run(self) -> Result<()> {
        let json = self.json;
        match self.execute().await {
            Ok(()) => Ok(()),
            Err(error) if json => {
                println!("{}", serde_json::json!({ "error": format!("{error:#}") }));
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

impl SlidesArgs {
    async fn execute(self) -> Result<()> {
        if self.instruction.trim().is_empty() {
            bail!("Instruction cannot be empty");
        }
        let model_overrides = parse_model_overrides(&self.model_overrides)?;
        let config = EffectiveConfig::load(ConfigOverrides {
            project: self.project.clone(),
            theme: self.theme,
            model_overrides: model_overrides.clone(),
            output_dir: self.out,
            pdf: self.pdf,
        })?;
        let request = GenerationRequest {
            instruction: self.instruction,
            sources: self.inputs,
            resource_kind: ResourceKind::Slides,
            project: self.project,
            model_overrides,
        };
        let result = generate_slides(
            config,
            request,
            GenerateSlidesOptions {
                title: self.title,
                dry_run: self.dry_run,
                event_sink: (!self.json && !self.dry_run)
                    .then_some(std::sync::Arc::new(render_generation_event)
                        as std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>),
            },
        )
        .await?;
        for warning in &result.warnings {
            eprintln!("{warning}");
        }

        if self.json {
            println!("{}", serde_json::to_string_pretty(&result.output)?);
        } else if self.dry_run {
            if !result.tool_summaries.is_empty() {
                println!("Injected tools:");
                for tool in &result.tool_summaries {
                    println!("- {}: {}", tool.name, tool.description);
                }
                println!();
            }
            if let Some(prompt) = result.prompt_preview {
                println!("{prompt}");
            }
            println!("Dry run complete; no files were written.");
        } else {
            println!("Wrote {}", result.markdown_path.display());
            if let Some(pdf_path) = result.pdf_path {
                println!("Wrote {}", pdf_path.display());
            }
        }
        Ok(())
    }
}

fn render_generation_event(event: TextGenerationEvent) {
    match event {
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
