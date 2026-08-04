use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use inquire::{Password, PasswordDisplayMode};
use serde_json::Value;

use crate::{
    cli::{
        ArtifactCommands, Commands, ConfigCommands, ConfigDeleteArgs, ConfigSetArgs,
        ConfigShowArgs, ConnectorCommands, ConnectorSetupArgs, ConnectorShowArgs, DocumentArgs,
        DocumentPageSizeArg, EditCommands, EditSlidesArgs, GenerateCommands, GenerationToolArg,
        InitProjectArgs, InitTarget, LocalVideoRendererArg, ModelAddArgs, ModelCommands,
        ModelEditArgs, ModelNameArgs, ModelUseArgs, PageArgs, PluginCommands, ProjectCommands,
        ProjectNameArgs, ProjectShowArgs, PromptCommands, PromptCustomizeArgs, PromptProjectArgs,
        PromptScope, PromptShowArgs, RendererCommands, SlidesArgs, TemplateCommands,
        TemplateKindArg, ThemeCommands, ThemeUseArgs, ToolCommands, VideoArgs, VideoAudioArg,
        VideoCommands, VideoEngineArg, VideoWorkflowArg,
    },
    commands::operation::interruptible,
    init::InitService,
    presentation::{Cell, print_table},
};
use sfumato_core::{
    application::{
        AddProjectAssetCommand, ApproveVideoReviewCommand, EditSlidesCommand,
        GenerateDocumentCommand, GeneratePageCommand, GenerateSlidesCommand, GenerateVideoCommand,
        PreviewVideoReviewCommand, SfumatoApplication, UpdateProjectAssetCommand,
    },
    config::{Capability, ConfigOverrides, GenerationToolKind, VideoAudioMode},
    config_editor::ConfigTarget,
    connectors::ConnectorPreset as CoreConnectorPreset,
    generation::{DocumentPageSize, GenerationRequest, ResourceKind},
    operation::OperationContext,
    prompts::{PromptOrigin, PromptOverrideScope},
    providers::TextGenerationEvent,
    resources::documents::GenerateDocumentResult,
    resources::pages::GeneratePageResult,
    resources::slides::{EditSlidesRequest, EditSlidesResult, GenerateSlidesResult},
    resources::videos::{GenerateVideoRequest, GenerateVideoResult},
    secrets::SecretValue,
    templates::TemplateKind,
};

pub(crate) mod operation;

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
            Self::Template { command } => command.run(application).await,
            Self::Artifact { command } => command.run(application).await,
            Self::Prompt { command } => command.run(application).await,
            Self::Plugin { command } => command.run(application).await,
            Self::Tool { command } => command.run(application).await,
            Self::Renderer { command } => command.run(application).await,
            Self::Video { command } => command.run(application).await,
            Self::Generate { command } => command.run(application).await,
            Self::Edit { command } => command.run(application).await,
        }
    }
}

#[async_trait]
impl RunnableCommand for VideoCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            // Both arms emit the same `{"error":{...}}` object the generate and
            // edit commands do. They used to propagate with `?`, leaving a
            // `--json` caller in the same command group with nothing parseable on
            // stdout and only prose on stderr.
            Self::Preview(args) => {
                let json = args.json;
                match application.preview_video_review(PreviewVideoReviewCommand {
                    config: ConfigOverrides {
                        project: args.project,
                        ..ConfigOverrides::default()
                    },
                    review_id: args.review_id,
                }) {
                    Ok(source) if json => {
                        println!("{}", serde_json::json!({"source": source}));
                    }
                    Ok(source) => {
                        println!(
                            "Review source: {}\nRun the managed Hyperframes preview from this directory, then approve the same review ID.",
                            source.display()
                        );
                    }
                    Err(error) if json => {
                        println!("{}", json_typed_error(&error));
                        return Err(error.into());
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Self::Approve(args) => {
                let json = args.json;
                match application
                    .approve_video_review(ApproveVideoReviewCommand {
                        operation: interruptible(),
                        config: ConfigOverrides {
                            project: args.project,
                            publish_dir: args.out,
                            ..ConfigOverrides::default()
                        },
                        review_id: args.review_id,
                    })
                    .await
                {
                    Ok(result) => render_video_result(result, json, false)?,
                    Err(error) if json => {
                        println!("{}", json_typed_error(&error));
                        return Err(error.into());
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ArtifactCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Add(args) => {
                let asset = application.add_project_asset(AddProjectAssetCommand {
                    source: args.path,
                    name: args.name,
                    description: args.description,
                    alt_text: args.alt_text,
                    tags: args.tags,
                    generation_prompt: args.prompt,
                    theme: args.theme,
                    all_themes: args.all_themes,
                    project: args.project,
                })?;
                println!(
                    "Added project artifact '{}' at {}",
                    asset.name,
                    asset
                        .variants
                        .values()
                        .next()
                        .map(|variant| variant.path.display().to_string())
                        .unwrap_or_default()
                );
            }
            Self::Edit(args) => {
                let generation_prompt = if args.clear_prompt {
                    Some(None)
                } else {
                    args.prompt.map(Some)
                };
                let variant_theme = args.from_theme.zip(args.to_theme);
                let tags = (!args.tags.is_empty()).then_some(args.tags);
                let asset = application.update_project_asset(UpdateProjectAssetCommand {
                    name: args.name,
                    description: args.description,
                    alt_text: args.alt_text,
                    tags,
                    generation_prompt,
                    variant_theme,
                    project: args.project,
                })?;
                println!("Updated project artifact '{}'", asset.name);
            }
            Self::List(args) => {
                let listing = application.list_project_assets(args.project.as_deref())?;
                if listing.entries.is_empty() && listing.is_complete() {
                    println!("No reusable project artifacts.");
                } else if !listing.entries.is_empty() {
                    print_table(
                        &["NAME", "THEMES", "TAGS", "DESCRIPTION"],
                        listing
                            .entries
                            .iter()
                            .map(|asset| {
                                vec![
                                    Cell::primary(&asset.name),
                                    Cell::new(
                                        asset
                                            .variants
                                            .keys()
                                            .cloned()
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                    ),
                                    Cell::muted(asset.metadata.tags.join(", ")),
                                    Cell::new(&asset.metadata.description),
                                ]
                            })
                            .collect(),
                    );
                }
                report_unreadable("project artifact", &listing.unreadable);
            }
            Self::Show(args) => {
                let asset = application.show_project_asset(&args.name, args.project.as_deref())?;
                println!(
                    "{}\nDescription: {}\nAlt text: {}\nTags: {}\nRegenerable: {}",
                    asset.name,
                    asset.metadata.description,
                    asset.metadata.alt_text,
                    asset.metadata.tags.join(", "),
                    asset.is_regenerable()
                );
                print_table(
                    &["THEME", "TYPE", "SHA-256", "FILE"],
                    asset
                        .variants
                        .into_values()
                        .map(|variant| {
                            vec![
                                Cell::primary(variant.theme),
                                Cell::new(variant.media_type),
                                Cell::muted(short_hash(&variant.content_hash)),
                                Cell::new(variant.path.display()),
                            ]
                        })
                        .collect(),
                );
            }
            Self::Remove(args) => {
                let asset =
                    application.remove_project_asset(&args.name, args.project.as_deref())?;
                println!("Removed project artifact '{}'", asset.name);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for TemplateCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::Create(args) => {
                let template = application.create_template(
                    &args.name,
                    template_kind(args.kind),
                    args.source,
                )?;
                println!(
                    "Created {} template '{}' at {}",
                    template.manifest.kind,
                    template.manifest.name,
                    template.root.display()
                );
            }
            Self::List(args) => {
                let listing = application.list_templates(args.kind.map(template_kind))?;
                if listing.entries.is_empty() && listing.is_complete() {
                    println!("No reusable generation templates installed.");
                } else if !listing.entries.is_empty() {
                    print_table(
                        &["NAME", "KIND", "DESCRIPTION"],
                        listing
                            .entries
                            .into_iter()
                            .map(|template| {
                                vec![
                                    Cell::primary(template.name),
                                    Cell::new(template.kind),
                                    Cell::new(template.description),
                                ]
                            })
                            .collect(),
                    );
                }
                report_unreadable("template", &listing.unreadable);
            }
            Self::Show(args) => {
                let template =
                    application.show_template(&args.name, args.kind.map(template_kind))?;
                println!(
                    "{} ({})\n{}\n\n{}",
                    template.manifest.name,
                    template.manifest.kind,
                    template.manifest.description,
                    template.source
                );
            }
        }
        Ok(())
    }
}

/// Shortens a content hash for display without assuming it is long enough.
///
/// These values come from persisted records — project-asset manifests, prompt
/// provenance — not from a digest just computed, so a hand-edited or corrupt file
/// carrying a short hash used to panic on the slice rather than produce an error.
fn short_hash(hash: &str) -> &str {
    const DISPLAYED_HASH_CHARS: usize = 12;
    // On a character boundary, so a non-hex value cannot panic here either.
    hash.get(..DISPLAYED_HASH_CHARS).unwrap_or(hash)
}

/// Warns about catalog entries that were skipped, naming each one.
///
/// On stderr rather than in the table: the listing's job is to show what is
/// usable, and a skipped entry is not. Silence would be worse than either — a
/// damaged package that stops appearing looks deleted rather than broken.
fn report_unreadable(label: &str, unreadable: &[sfumato_core::catalogs::UnreadableEntry]) {
    for entry in unreadable {
        eprintln!(
            "warning: skipped {label} '{}': {}",
            entry.name, entry.problem
        );
    }
}

fn template_kind(kind: TemplateKindArg) -> TemplateKind {
    match kind {
        TemplateKindArg::Slides => TemplateKind::Slides,
        TemplateKindArg::Page => TemplateKind::Page,
        TemplateKindArg::Document => TemplateKind::Document,
    }
}

#[async_trait]
impl RunnableCommand for PluginCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::List(args) => {
                let listing = application
                    .list_page_plugins(args.project.as_deref(), &interruptible())
                    .await?;
                report_unreadable("page plugin", &listing.unreadable);
                print_table(
                    &["ID", "PLUGIN", "LATEST", "INSTALLED", "PROJECT"],
                    listing
                        .entries
                        .into_iter()
                        .map(|status| {
                            vec![
                                Cell::primary(status.plugin.id),
                                Cell::new(status.plugin.name),
                                Cell::muted(status.plugin.latest_version),
                                match status.installed_version {
                                    Some(version) => Cell::success(version),
                                    None => Cell::warning("not installed"),
                                },
                                if status.enabled {
                                    Cell::success("enabled")
                                } else {
                                    Cell::muted("disabled")
                                },
                            ]
                        })
                        .collect(),
                );
                Ok(())
            }
            Self::Show(args) => {
                let status = application
                    .show_page_plugin_status(&args.id, None, &interruptible())
                    .await?;
                println!(
                    "{}\nID: {}\nLatest: {}\nInstalled: {}\nEnabled: {}\n\n{}",
                    status.plugin.name,
                    status.plugin.id,
                    status.plugin.latest_version,
                    status.installed_version.as_deref().unwrap_or("no"),
                    if status.enabled { "yes" } else { "no" },
                    status.plugin.description,
                );
                if let Ok(plugin) = application.show_page_plugin(&args.id) {
                    println!(
                        "\nAPI: {}\nSHA-256: {}\nLicense: {}\n\n{}",
                        plugin.summary.api_global,
                        plugin.summary.runtime_hash,
                        plugin.summary.license,
                        plugin.guidance,
                    );
                } else {
                    println!(
                        "\nInstall with: sfumato plugin install {}",
                        status.plugin.id
                    );
                }
                Ok(())
            }
            Self::Install(args) => {
                let result = application
                    .install_page_plugin(&args.id, args.version.as_deref(), &interruptible())
                    .await?;
                for package in result.packages {
                    println!("Installed {} {}", package.id, package.version);
                }
                Ok(())
            }
            Self::Update(args) => {
                let result = application
                    .update_page_plugin(&args.id, &interruptible())
                    .await?;
                for package in result.packages {
                    println!("Updated {} to {}", package.id, package.version);
                }
                Ok(())
            }
            Self::Enable(args) => {
                let changed = application.enable_page_plugin(&args.id, args.project.as_deref())?;
                println!("Enabled {} for project {}", changed.plugin, changed.project);
                Ok(())
            }
            Self::Disable(args) => {
                let changed = application.disable_page_plugin(&args.id, args.project.as_deref())?;
                println!(
                    "Disabled {} for project {}",
                    changed.plugin, changed.project
                );
                Ok(())
            }
        }
    }
}

#[async_trait]
impl RunnableCommand for ToolCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        match self {
            Self::List(args) => {
                let statuses = application.list_generation_tools(args.project)?;
                print_table(
                    &["TOOL", "STATUS", "MODEL"],
                    statuses
                        .into_iter()
                        .map(|status| {
                            vec![
                                Cell::primary(status.tool.as_str()),
                                if status.enabled {
                                    Cell::success("enabled")
                                } else {
                                    Cell::muted("disabled")
                                },
                                if status.model_configured {
                                    Cell::success("configured")
                                } else {
                                    Cell::warning("missing")
                                },
                            ]
                        })
                        .collect(),
                );
            }
            Self::Enable(args) => {
                let tool = generation_tool(args.tool);
                let project =
                    application.set_generation_tool(tool, true, args.project.as_deref())?;
                println!("Enabled {} for project {}", tool.as_str(), project.name);
            }
            Self::Disable(args) => {
                let tool = generation_tool(args.tool);
                let project =
                    application.set_generation_tool(tool, false, args.project.as_deref())?;
                println!("Disabled {} for project {}", tool.as_str(), project.name);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for RendererCommands {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let operation = interruptible();
        let statuses = match self {
            Self::List => application.list_renderers(&operation).await?,
            Self::Install(args) => vec![
                application
                    .install_renderer(local_renderer(args.renderer), &operation)
                    .await?,
            ],
            Self::Remove(args) => vec![application.remove_renderer(local_renderer(args.renderer))?],
            Self::Doctor(args) => {
                application
                    .doctor_renderers(args.renderer.map(local_renderer), &operation)
                    .await?
            }
        };
        print_renderer_statuses(statuses);
        Ok(())
    }
}

fn print_renderer_statuses(statuses: Vec<sfumato_core::renderers::RendererStatus>) {
    print_table(
        &["RENDERER", "VERSION", "INSTALLED", "HEALTH", "DETAILS"],
        statuses
            .into_iter()
            .map(|status| {
                vec![
                    Cell::primary(status.id),
                    Cell::muted(status.version),
                    if status.installed {
                        Cell::success("yes")
                    } else {
                        Cell::warning("no")
                    },
                    if status.healthy {
                        Cell::success("healthy")
                    } else {
                        Cell::warning("unavailable")
                    },
                    Cell::new(status.details.join("; ")),
                ]
            })
            .collect(),
    );
}

fn generation_tool(tool: GenerationToolArg) -> GenerationToolKind {
    match tool {
        GenerationToolArg::ImageGen => GenerationToolKind::ImageGen,
        GenerationToolArg::VideoGen => GenerationToolKind::VideoGen,
        GenerationToolArg::AudioGen => GenerationToolKind::AudioGen,
        GenerationToolArg::ChartGen => GenerationToolKind::ChartGen,
    }
}

fn local_renderer(renderer: LocalVideoRendererArg) -> &'static str {
    match renderer {
        LocalVideoRendererArg::Hyperframe => "hyperframe",
        LocalVideoRendererArg::Manim => "manim",
        LocalVideoRendererArg::PagedJs => "pagedjs",
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
        print_table(
            &["PROMPT", "ORIGIN"],
            application
                .list_prompts(self.project)?
                .into_iter()
                .map(|prompt| {
                    vec![
                        Cell::primary(prompt.id),
                        Cell::muted(prompt_origin_label(&prompt.provenance.origin)),
                    ]
                })
                .collect(),
        );
        Ok(())
    }

    fn validate(self, application: &SfumatoApplication) -> Result<()> {
        let validation = application.validate_prompts(self.project)?;
        println!("Validated {} prompt templates.", validation.resolved.len());
        for prompt in validation.resolved {
            println!(
                "{}\t{}\t{}",
                prompt.id,
                prompt_origin_label(&prompt.origin),
                short_hash(&prompt.content_hash)
            );
        }
        // Reporting success while an override sits unused is the whole defect:
        // the edit changes nothing and the command that checks prompts says the
        // setup is fine.
        for stray in &validation.unreferenced {
            eprintln!(
                "warning: {} is not a prompt template and is being ignored.{}",
                stray.path.display(),
                match &stray.expected {
                    Some(expected) => format!(" Did you mean {}?", expected.display()),
                    None => String::new(),
                }
            );
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for PromptShowArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let id = application.parse_prompt_id(&self.id)?;
        let source = application.show_prompt(&self.id, self.project)?;
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
        let scope = match self.scope {
            PromptScope::User => PromptOverrideScope::User,
            PromptScope::Project => PromptOverrideScope::Project,
        };
        let path = application.customize_prompt(&self.id, scope, self.project)?;
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
                } else {
                    print_table(
                        &["PROFILE", "CONNECTOR", "MODEL", "CAPABILITIES"],
                        models
                            .into_iter()
                            .map(|model| {
                                vec![
                                    Cell::primary(model.name),
                                    Cell::new(model.connector),
                                    Cell::new(model.model),
                                    Cell::muted(
                                        model
                                            .capabilities
                                            .iter()
                                            .map(|capability| capability.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                    ),
                                ]
                            })
                            .collect(),
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
            Self::Import(args) => {
                let theme = application.import_theme_design(args.path, args.name.as_deref())?;
                println!(
                    "Imported DESIGN.md as theme '{}' at {}",
                    theme.manifest.name,
                    theme.root.display()
                );
                Ok(())
            }
            Self::Export(args) => {
                let path = application.export_theme_design(&args.name, args.out)?;
                println!("Exported theme '{}' to {}", args.name, path.display());
                Ok(())
            }
            Self::List => {
                print_table(
                    &["THEME"],
                    application
                        .list_themes()?
                        .into_iter()
                        .map(|theme| vec![Cell::primary(theme.name)])
                        .collect(),
                );
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
                print_table(
                    &["CONNECTOR", "KIND", "TARGET"],
                    application
                        .list_connectors()?
                        .into_iter()
                        .map(|connector| {
                            vec![
                                Cell::primary(connector.name),
                                Cell::new(connector.kind),
                                Cell::muted(connector.target),
                            ]
                        })
                        .collect(),
                );
                Ok(())
            }
            Self::Presets => {
                print_table(
                    &["PRESET", "KIND", "TRANSPORT", "AUTHENTICATION"],
                    CoreConnectorPreset::ALL
                        .into_iter()
                        .map(|preset| {
                            vec![
                                Cell::primary(preset.as_str()),
                                Cell::new(preset.kind()),
                                Cell::muted(preset.transport_summary()),
                                Cell::muted(preset.auth_summary()),
                            ]
                        })
                        .collect(),
                );
                Ok(())
            }
            Self::Show(args) => args.run(Arc::clone(&application)).await,
            Self::Capabilities(args) => {
                let capabilities = application.connector_capabilities(&args.name)?;
                print_table(
                    &["CONNECTOR KIND", "NATIVE FEATURE"],
                    capabilities
                        .features
                        .into_iter()
                        .map(|feature| {
                            vec![
                                Cell::primary(&capabilities.kind),
                                Cell::new(feature.as_str()),
                            ]
                        })
                        .collect(),
                );
                Ok(())
            }
            Self::Models(args) => {
                let models = application
                    .list_connector_models(&args.name, interruptible())
                    .await?;
                print_table(
                    &["DEFAULT", "MODEL", "NAME", "INPUTS", "OUTPUTS", "CONTEXT"],
                    models
                        .into_iter()
                        .filter(|model| !model.hidden)
                        .map(|model| {
                            vec![
                                if model.is_default {
                                    Cell::success("default")
                                } else {
                                    Cell::muted("")
                                },
                                Cell::primary(model.id),
                                Cell::new(model.display_name),
                                Cell::muted(model.input_modalities.join(", ")),
                                Cell::muted(model.output_modalities.join(", ")),
                                Cell::muted(
                                    model
                                        .context_length
                                        .map(|value| value.to_string())
                                        .unwrap_or_default(),
                                ),
                            ]
                        })
                        .collect(),
                );
                Ok(())
            }
            Self::Status(args) => {
                let status = application
                    .connector_status(&args.name, interruptible())
                    .await?;
                print_table(
                    &["CONNECTOR", "KIND", "FIELD", "VALUE"],
                    status
                        .fields
                        .into_iter()
                        .map(|field| {
                            vec![
                                Cell::primary(&status.connector),
                                Cell::muted(&status.kind),
                                Cell::new(field.name),
                                Cell::new(field.value),
                            ]
                        })
                        .collect(),
                );
                Ok(())
            }
            Self::Setup(args) => args.run(application).await,
            Self::Login(args) => {
                let secret = Password::new(&format!("API key for {}", args.name))
                    .without_confirmation()
                    .with_display_mode(PasswordDisplayMode::Hidden)
                    .prompt()?;
                let status = application
                    .login_connector(&args.name, SecretValue::new(secret))
                    .await?;
                println!(
                    "Stored credential securely for connector '{}' ({})",
                    status.name,
                    status.credential.as_deref().unwrap_or("stored")
                );
                Ok(())
            }
            Self::AuthStatus(args) => {
                let status = application.connector_auth_status(&args.name).await?;
                if status.managed_externally {
                    println!(
                        "Connector '{}': authentication is managed by Codex CLI; run `codex login status`",
                        status.name
                    );
                    return Ok(());
                }
                println!(
                    "Connector '{}': {}{}",
                    status.name,
                    if status.available {
                        "credential available"
                    } else {
                        "credential unavailable"
                    },
                    status
                        .credential
                        .as_deref()
                        .map(|reference| format!(" ({reference})"))
                        .unwrap_or_default()
                );
                Ok(())
            }
            Self::Logout(args) => {
                let status = application.logout_connector(&args.name).await?;
                println!("Removed credential for connector '{}'", status.name);
                Ok(())
            }
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
        let connector =
            application.setup_connector(self.preset.into(), self.name, self.api_key_env)?;
        println!(
            "Configured {} connector '{}'",
            connector.kind, connector.name
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
                } else {
                    print_table(
                        &["STATUS", "PROJECT", "PATH"],
                        projects
                            .into_iter()
                            .map(|project| {
                                vec![
                                    match (project.available, project.active) {
                                        // A missing path outranks active: it is
                                        // the state that will break the next
                                        // command run against this project.
                                        (false, _) => Cell::warning("missing"),
                                        (true, true) => Cell::success("active"),
                                        (true, false) => Cell::muted(""),
                                    },
                                    Cell::primary(project.name),
                                    Cell::new(project.path.display()),
                                ]
                            })
                            .collect(),
                    );
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
            Self::Document(args) => args.run(application).await,
            Self::Page(args) => args.run(application).await,
            Self::Video(args) => args.run(application).await,
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
        match execute_page(&application, self, event_sink, interruptible()).await {
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
        pdf: None,
        tool_overrides: parse_tool_overrides(&args.tools, &args.disabled_tools)?,
    };
    let request = GenerationRequest {
        instruction: args.instruction,
        sources: args.inputs,
        resource_kind: ResourceKind::Page,
        project: args.project,
        model_overrides,
    };
    let plugins = args.plugins;
    let ui = if args.shadcn {
        eprintln!("Warning: --shadcn is deprecated; use --ui shadcn.");
        Some("shadcn".to_string())
    } else {
        args.ui
            .map(|ui| if ui == "none" { String::new() } else { ui })
    };
    Ok(application
        .generate_page(GeneratePageCommand {
            operation,
            config,
            request,
            title: args.title,
            template: args.template,
            plugins,
            disabled_plugins: args.disabled_plugins,
            ui,
            dry_run: args.dry_run,
            review: !args.no_review,
            event_sink,
        })
        .await?)
}

#[async_trait]
impl RunnableCommand for VideoArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let json = self.json;
        let dry_run = self.dry_run;
        let event_sink =
            (!json && !dry_run)
                .then_some(Arc::new(render_generation_event)
                    as Arc<dyn Fn(TextGenerationEvent) + Send + Sync>);
        match execute_video(&application, self, event_sink, interruptible()).await {
            Ok(result) => render_video_result(result, json, dry_run),
            Err(error) if json => {
                println!("{}", json_operation_error(&error));
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

pub(crate) async fn execute_video(
    application: &SfumatoApplication,
    args: VideoArgs,
    event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: OperationContext,
) -> Result<GenerateVideoResult> {
    if args.instruction.trim().is_empty() {
        bail!("Instruction cannot be empty");
    }
    let model_overrides = parse_model_overrides(&args.model_overrides)?;
    let engine = match args.engine {
        VideoEngineArg::Hyperframe => sfumato_core::renderers::VideoEngine::Hyperframe,
        VideoEngineArg::Manim => sfumato_core::renderers::VideoEngine::Manim,
        VideoEngineArg::Model => sfumato_core::renderers::VideoEngine::Model,
    };
    let local = !matches!(engine, sfumato_core::renderers::VideoEngine::Model);
    if local && model_overrides.contains_key(&Capability::Video) {
        bail!("--model video=... is only valid with --engine model");
    }
    if !local && model_overrides.contains_key(&Capability::Code) {
        bail!("--model code=... is only valid with --engine hyperframe or --engine manim");
    }
    if !local && (args.fps.is_some() || args.quality.is_some()) {
        bail!("--fps and --quality are only valid with Hyperframe or Manim");
    }
    // Manim is not the only engine that runs generated Python: a Hyperframe film
    // with chart-gen enabled runs it too, and rejecting the flag there left that
    // tool permanently unreachable. The flag is refused only for `--engine model`,
    // which runs no local code at all, so consenting to it would mean nothing.
    if args.allow_code_execution && matches!(engine, sfumato_core::renderers::VideoEngine::Model) {
        bail!("--allow-code-execution is not valid with --engine model, which runs no local code");
    }
    if args.visual_review && !matches!(engine, sfumato_core::renderers::VideoEngine::Hyperframe) {
        bail!("--visual-review is only valid with --engine hyperframe");
    }
    if !args.urls.is_empty() && !matches!(engine, sfumato_core::renderers::VideoEngine::Hyperframe)
    {
        bail!("--url is only valid with --engine hyperframe");
    }
    let resolution = args
        .resolution
        .unwrap_or_else(|| if local { "1080p".into() } else { "720p".into() });
    let aspect_ratio = args.aspect_ratio.unwrap_or_else(|| "16:9".into());
    let audio = match args.audio {
        Some(VideoAudioArg::Auto) => VideoAudioMode::Auto,
        Some(VideoAudioArg::On) => VideoAudioMode::On,
        Some(VideoAudioArg::Off) => VideoAudioMode::Off,
        // A local engine narrates when the project can: a configured speech
        // profile is the opt-in, the same way a configured image profile enables
        // illustrations. Sfumato owns both local timelines, so both can be
        // retimed around the voice.
        None => VideoAudioMode::Auto,
    };
    if args.voice.is_some() && matches!(engine, sfumato_core::renderers::VideoEngine::Model) {
        bail!("--voice is only valid with a local engine; a remote model picks its own voice");
    }
    let config = ConfigOverrides {
        project: args.project.clone(),
        theme: args.theme,
        model_overrides: model_overrides.clone(),
        reviewer_model: args.review_model,
        publish_dir: args.out,
        pdf: None,
        tool_overrides: parse_tool_overrides(&args.tools, &args.disabled_tools)?,
    };
    let request = GenerationRequest {
        instruction: args.instruction,
        sources: args.inputs,
        resource_kind: ResourceKind::Video,
        project: args.project,
        model_overrides,
    };
    Ok(application
        .generate_video(GenerateVideoCommand {
            operation,
            config,
            request,
            video: GenerateVideoRequest {
                engine,
                title: args.title,
                duration_seconds: args.duration,
                resolution,
                aspect_ratio,
                fps: args.fps.unwrap_or(30),
                quality: args.quality.unwrap_or_else(|| "high".into()),
                audio,
                voice: args.voice,
                allow_code_execution: args.allow_code_execution,
                workflow: match args.workflow {
                    VideoWorkflowArg::Auto => sfumato_domain::VideoWorkflow::Auto,
                    VideoWorkflowArg::Explainer => sfumato_domain::VideoWorkflow::Explainer,
                    VideoWorkflowArg::MotionGraphics => {
                        sfumato_domain::VideoWorkflow::MotionGraphics
                    }
                    VideoWorkflowArg::ProductLaunch => sfumato_domain::VideoWorkflow::ProductLaunch,
                    VideoWorkflowArg::TalkingHead => sfumato_domain::VideoWorkflow::TalkingHead,
                    VideoWorkflowArg::Slideshow => sfumato_domain::VideoWorkflow::Slideshow,
                    VideoWorkflowArg::General => sfumato_domain::VideoWorkflow::General,
                },
                urls: args.urls,
                visual_review: args.visual_review,
            },
            dry_run: args.dry_run,
            review: !args.no_review,
            event_sink,
        })
        .await?)
}

fn render_video_result(result: GenerateVideoResult, json: bool, dry_run: bool) -> Result<()> {
    for warning in &result.output.warnings {
        eprintln!("{warning}");
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&result.output)?);
    } else if dry_run {
        println!("Project: {}", result.output.project);
        println!("Engine: {:?}", result.output.engine);
        println!("Planned MP4: {}", result.video_path.display());
        if !result.output.models.is_empty() {
            println!("Models:");
            for (role, profile) in &result.output.models {
                println!("- {role}: {profile}");
            }
        }
        if !result.output.tools.is_empty() {
            println!("Injected tools:");
            for tool in &result.output.tools {
                println!("- {}: {}", tool.name, tool.description);
            }
        }
        if let Some(prompt) = result.prompt_preview {
            println!("\n{prompt}");
        }
        println!("Dry run complete; no model, renderer, or artifact store was called.");
    } else {
        if let Some(session) = &result.output.review_session {
            println!("Visual review ready: {}", session.review_id);
            println!("Contact sheet: {}", result.video_path.display());
            println!("Next: sfumato video preview {}", session.review_id);
            println!("Then: sfumato video approve {}", session.review_id);
        } else {
            println!("Wrote {}", result.video_path.display());
        }
        if let Some(narration) = &result.output.narration {
            println!(
                "Narrated {} passages in {:.1}s with {} ({} caption groups)",
                narration.segments,
                narration.spoken_seconds,
                narration.profile,
                narration.caption_groups
            );
        }
        for path in result.published_paths {
            println!("Published {}", path.display());
        }
    }
    Ok(())
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
        println!(
            "Template: {}",
            result.output.template.as_deref().unwrap_or("none")
        );
        if !result.output.project_assets.is_empty() {
            println!("Reusable project artifacts:");
            for asset in &result.output.project_assets {
                println!("- {}: {}", asset.name, asset.reference);
            }
        }
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
        match execute_edit_slides(&application, self, event_sink, interruptible()).await {
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
        pdf: Some(true),
        tool_overrides: BTreeMap::new(),
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
        match execute_slides(&application, self, event_sink, interruptible()).await {
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

#[async_trait]
impl RunnableCommand for DocumentArgs {
    async fn run(self, application: Arc<SfumatoApplication>) -> Result<()> {
        let json = self.json;
        let dry_run = self.dry_run;
        let event_sink = (!json && !dry_run)
            .then_some(std::sync::Arc::new(render_generation_event)
                as std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>);
        match execute_document(&application, self, event_sink, interruptible()).await {
            Ok(result) => {
                render_document_result(result, json, dry_run)?;
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

pub(crate) async fn execute_document(
    application: &SfumatoApplication,
    args: DocumentArgs,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: OperationContext,
) -> Result<GenerateDocumentResult> {
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
        pdf: None,
        tool_overrides: parse_tool_overrides(&args.tools, &args.disabled_tools)?,
    };
    let request = GenerationRequest {
        instruction: args.instruction,
        sources: args.inputs,
        resource_kind: ResourceKind::Document,
        project: args.project,
        model_overrides,
    };
    Ok(application
        .generate_document(GenerateDocumentCommand {
            operation,
            config,
            request,
            title: args.title,
            template: args.template,
            page_size: args.page_size.map(|value| match value {
                DocumentPageSizeArg::A4 => DocumentPageSize::A4,
                DocumentPageSizeArg::Letter => DocumentPageSize::Letter,
            }),
            // Absent means "let the theme decide", so only an explicit flag
            // becomes an override.
            table_of_contents: flag_override(args.toc, args.no_toc),
            cover: flag_override(args.cover, args.no_cover),
            dry_run: args.dry_run,
            review: !args.no_review,
            event_sink,
        })
        .await?)
}

/// Turns a `--flag` / `--no-flag` pair into an explicit override.
fn flag_override(enabled: bool, disabled: bool) -> Option<bool> {
    match (enabled, disabled) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

fn render_document_result(result: GenerateDocumentResult, json: bool, dry_run: bool) -> Result<()> {
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
        println!(
            "Template: {}",
            result.output.template.as_deref().unwrap_or("none")
        );
        let setup = result.output.page_setup;
        println!(
            "Page: {}, cover {}, contents {}",
            setup.page_size.as_str(),
            if setup.cover { "on" } else { "off" },
            if setup.table_of_contents { "on" } else { "off" }
        );
        if !result.output.project_assets.is_empty() {
            println!("Reusable project artifacts:");
            for asset in &result.output.project_assets {
                println!("- {}: {}", asset.name, asset.reference);
            }
            println!();
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
                "Review: enabled with model profile '{reviewer}' (semantic review and conditional page-format repair)."
            );
        } else {
            println!("Review: disabled.");
        }
        println!(
            "Planned PDF: {}",
            result.markdown_path.with_extension("pdf").display()
        );
        println!("Dry run complete; no files were written.");
    } else {
        println!("Wrote {}", result.markdown_path.display());
        if let Some(pdf_path) = &result.pdf_path {
            println!("Wrote {}", pdf_path.display());
        }
        if let Some(published) = &result.published_pdf_path {
            println!("Published {}", published.display());
        }
    }
    Ok(())
}

fn json_operation_error(error: &anyhow::Error) -> serde_json::Value {
    error
        .downcast_ref::<sfumato_core::errors::SfumatoError>()
        .map(json_typed_error)
        .unwrap_or_else(|| serde_json::json!({ "error": { "message": format!("{error:#}") } }))
}

/// Renders the `--json` error object for an already-typed failure.
///
/// The video review commands return `SfumatoError` directly rather than through
/// `anyhow`, and they emit the same object as every other `--json` command.
fn json_typed_error(error: &sfumato_core::errors::SfumatoError) -> serde_json::Value {
    serde_json::json!({ "error": error })
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
        pdf: flag_override(args.pdf, args.no_pdf),
        tool_overrides: parse_tool_overrides(&args.tools, &args.disabled_tools)?,
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
            template: args.template,
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
        println!(
            "Template: {}",
            result.output.template.as_deref().unwrap_or("none")
        );
        if !result.output.project_assets.is_empty() {
            println!("Reusable project artifacts:");
            for asset in &result.output.project_assets {
                println!("- {}: {}", asset.name, asset.reference);
            }
            println!();
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
        TextGenerationEvent::ModelSelected {
            model,
            display_name,
        } => {
            eprintln!(
                "{} {} {}",
                styled_label("model", ANSI_CYAN),
                bold(&display_name),
                dim(&format!("({model})"))
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
        TextGenerationEvent::SourceRepairStarted { reason, scene } => {
            let target = scene
                .map(|scene| format!(" {}", bold(&scene)))
                .unwrap_or_else(|| " whole source".to_string());
            eprintln!(
                "{} repairing{target}: {}",
                styled_label("repair", ANSI_YELLOW),
                dim(&reason)
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

fn parse_tool_overrides(
    enabled: &[GenerationToolArg],
    disabled: &[GenerationToolArg],
) -> Result<BTreeMap<GenerationToolKind, bool>> {
    let mut overrides = BTreeMap::new();
    for tool in enabled {
        overrides.insert(generation_tool(*tool), true);
    }
    for tool in disabled {
        let tool = generation_tool(*tool);
        if overrides.contains_key(&tool) {
            bail!(
                "Generation tool '{}' cannot be both enabled and disabled",
                tool.as_str()
            );
        }
        overrides.insert(tool, false);
    }
    Ok(overrides)
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/commands.rs"]
mod tests;
