//! Asynchronous effects that bridge core operations into TUI messages.

use super::*;

pub(super) fn operation_event_sink(job_id: u64, sender: Sender<UiMessage>) -> Arc<dyn EventSink> {
    Arc::new(UiOperationEventSink { job_id, sender })
}

pub(super) fn generation_event_sink(
    job_id: u64,
    sender: Sender<UiMessage>,
) -> Arc<dyn Fn(TextGenerationEvent) + Send + Sync> {
    Arc::new(move |event| {
        let _ = sender.try_send(UiMessage::GenerationEvent { job_id, event });
    })
}

pub(super) fn spawn_generation(
    job_id: u64,
    application: Arc<SfumatoApplication>,
    args: SlidesArgs,
    sink: Arc<dyn Fn(TextGenerationEvent) + Send + Sync>,
    operation: OperationContext,
    sender: Sender<UiMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match execute_slides(&application, args, Some(sink), operation).await {
            Err(error) if is_cancelled_error(&error) => {
                let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
            }
            result => {
                let result = result
                    .map(ResourceResult::Generated)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender
                    .send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    })
                    .await;
            }
        }
    })
}

pub(super) fn spawn_page_generation(
    job_id: u64,
    application: Arc<SfumatoApplication>,
    args: PageArgs,
    sink: Arc<dyn Fn(TextGenerationEvent) + Send + Sync>,
    operation: OperationContext,
    sender: Sender<UiMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match execute_page(&application, args, Some(sink), operation).await {
            Err(error) if is_cancelled_error(&error) => {
                let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
            }
            result => {
                let result = result
                    .map(ResourceResult::GeneratedPage)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender
                    .send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    })
                    .await;
            }
        }
    })
}

pub(super) fn spawn_video_generation(
    job_id: u64,
    application: Arc<SfumatoApplication>,
    args: VideoArgs,
    sink: Arc<dyn Fn(TextGenerationEvent) + Send + Sync>,
    operation: OperationContext,
    sender: Sender<UiMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match execute_video(&application, args, Some(sink), operation).await {
            Err(error) if is_cancelled_error(&error) => {
                let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
            }
            result => {
                let result = result
                    .map(ResourceResult::GeneratedVideo)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender
                    .send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    })
                    .await;
            }
        }
    })
}

pub(super) fn spawn_edit(
    job_id: u64,
    application: Arc<SfumatoApplication>,
    args: EditSlidesArgs,
    sink: Arc<dyn Fn(TextGenerationEvent) + Send + Sync>,
    operation: OperationContext,
    sender: Sender<UiMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match execute_edit_slides(&application, args, Some(sink), operation).await {
            Err(error) if is_cancelled_error(&error) => {
                let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
            }
            result => {
                let result = result
                    .map(ResourceResult::Edited)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender
                    .send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    })
                    .await;
            }
        }
    })
}

/// Deadline for one native connector read started from the browse view.
///
/// Catalog and status reads answer in well under a second, and the view has
/// nothing to show until they return. Matching the adapters' introspection
/// timeout keeps the deadline from firing before the transport's own bound, so
/// the user sees the transport's error rather than a bare deadline.
const CONNECTOR_QUERY_DEADLINE: Duration = Duration::from_secs(60);

/// Starts a native connector read, returning its cancellation and task handles.
///
/// Both paths used to build `OperationContext::detached()` — no deadline and a
/// token nobody signalled — and the task was dropped on the floor, so an
/// unresponsive endpoint hung the view with `Esc` doing nothing.
pub(super) fn spawn_connector_query(
    application: Arc<SfumatoApplication>,
    connector: String,
    models: bool,
    sender: Sender<UiMessage>,
) -> ConnectorQuery {
    let (cancellation, operation) =
        OperationContext::create(Some(CONNECTOR_QUERY_DEADLINE), Arc::new(DiscardEvents));
    let task = tokio::spawn(async move {
        let result = if models {
            application
                .list_connector_models(&connector, operation)
                .await
                .map(|models| {
                    models
                        .into_iter()
                        .filter(|model| !model.hidden)
                        .map(|model| {
                            let mut detail = model.description.unwrap_or_default();
                            if !model.metadata.is_empty() {
                                detail.push_str("\n\nMetadata:\n");
                                for (name, value) in model.metadata {
                                    detail.push_str(&format!("- {name}: {value}\n"));
                                }
                            }
                            BrowseRow {
                                title: model.id,
                                subtitle: format!(
                                    "{} -> {}{}",
                                    model.input_modalities.join(", "),
                                    model.output_modalities.join(", "),
                                    model
                                        .context_length
                                        .map(|value| format!(" / {value} tokens"))
                                        .unwrap_or_default(),
                                ),
                                detail,
                                active: model.is_default,
                            }
                        })
                        .collect()
                })
        } else {
            application
                .connector_status(&connector, operation)
                .await
                .map(|status| {
                    status
                        .fields
                        .into_iter()
                        .map(|field| BrowseRow {
                            title: field.name,
                            subtitle: field.value.clone(),
                            detail: format!(
                                "{} / {}\n\n{}",
                                status.connector, status.kind, field.value
                            ),
                            active: false,
                        })
                        .collect()
                })
        }
        .map_err(|error| error.to_string());
        let _ = sender
            .send(UiMessage::ConnectorQueryFinished { connector, result })
            .await;
    });
    ConnectorQuery::new(cancellation, task)
}

struct UiOperationEventSink {
    job_id: u64,
    sender: Sender<UiMessage>,
}

impl EventSink for UiOperationEventSink {
    fn try_emit(&self, event: OperationEvent) -> std::result::Result<(), EventSinkError> {
        self.sender
            .try_send(UiMessage::OperationEvent {
                job_id: self.job_id,
                event,
            })
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => EventSinkError::Full,
                tokio::sync::mpsc::error::TrySendError::Closed(_) => EventSinkError::Closed,
            })
    }
}

fn is_cancelled_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<SfumatoError>()
        .is_some_and(|error| error.class == ErrorClass::Cancelled)
}

pub(super) fn execute_operation(
    form: &OperationForm,
    application: &SfumatoApplication,
) -> Result<String> {
    match form.kind {
        OperationKind::ProjectCreate => {
            let name = required_field(form, "Name")?;
            let path = PathBuf::from(required_field(form, "Path")?);
            let project = application.init_project(name, path, form.toggle("Make active"))?;
            Ok(format!("Created project '{}'", project.name))
        }
        OperationKind::ProjectRemove => {
            if !form.toggle("Confirm") {
                anyhow::bail!("Confirm removal before continuing");
            }
            let name = form.target.as_deref().context("Project name is missing")?;
            let removed = application.remove_project(name)?;
            Ok(format!(
                "Removed project '{}' from the registry",
                removed.name
            ))
        }
        OperationKind::ModelAdd => {
            let name = required_field(form, "Name")?;
            application.add_model(
                name.clone(),
                required_field(form, "Connector")?,
                required_field(form, "Model ID")?,
                split_values(&required_field(form, "Capabilities")?),
                split_values(&form.text("Options")),
            )?;
            Ok(format!("Added model profile '{name}'"))
        }
        OperationKind::ModelEdit => {
            let name = form
                .target
                .as_deref()
                .context("Model profile name is missing")?;
            application.edit_model(
                name,
                Some(required_field(form, "Connector")?),
                Some(required_field(form, "Model ID")?),
                split_values(&required_field(form, "Capabilities")?),
                split_values(&form.text("Options")),
            )?;
            Ok(format!("Updated model profile '{name}'"))
        }
        OperationKind::ModelUse => {
            let changed = application.use_model(
                &required_field(form, "Capability or role")?,
                &required_field(form, "Profile")?,
                optional_field(form, "Project").as_deref(),
            )?;
            Ok(format!(
                "'{}' now uses model profile '{}'",
                changed.selection.as_str(),
                changed.profile
            ))
        }
        OperationKind::ModelRemove => {
            if !form.toggle("Confirm") {
                anyhow::bail!("Confirm removal before continuing");
            }
            let name = form
                .target
                .as_deref()
                .context("Model profile name is missing")?;
            application.remove_model(name)?;
            Ok(format!("Removed model profile '{name}'"))
        }
        OperationKind::ConnectorSetup => {
            let preset = ConnectorPreset::from_str(&form.select("Preset"))?;
            let connector = application.setup_connector(
                preset,
                optional_field(form, "Name"),
                optional_field(form, "API key environment"),
            )?;
            Ok(format!("Configured connector '{}'", connector.name))
        }
        OperationKind::ThemeCreate => {
            let name = required_field(form, "Name")?;
            application.create_theme(&name)?;
            Ok(format!("Created theme '{name}'"))
        }
        OperationKind::ThemeImport => {
            let path = PathBuf::from(required_field(form, "Path")?);
            let name = optional_field(form, "Name");
            let theme = application.import_theme_design(path, name.as_deref())?;
            Ok(format!("Imported theme '{}'", theme.manifest.name))
        }
        OperationKind::ThemeExport => {
            let name = form.target.as_deref().context("Theme name is missing")?;
            let path = application
                .export_theme_design(name, PathBuf::from(required_field(form, "Path")?))?;
            Ok(format!("Exported theme '{}' to {}", name, path.display()))
        }
        OperationKind::ThemeUse => {
            let name = form.target.as_deref().context("Theme name is missing")?;
            let project = optional_field(form, "Project");
            let updated = application.use_theme(name, project.as_deref())?;
            Ok(format!(
                "Project '{}' now uses theme '{}'",
                updated.name, updated.theme
            ))
        }
        OperationKind::TemplateCreate => {
            let name = required_field(form, "Name")?;
            let kind = TemplateKind::from_str(&required_field(form, "Kind")?)?;
            let source = optional_field(form, "Source").map(PathBuf::from);
            application.create_template(&name, kind, source)?;
            Ok(format!("Created {kind} template '{name}'"))
        }
        OperationKind::ArtifactAdd => {
            let path = PathBuf::from(required_field(form, "Path")?);
            let name = optional_field(form, "Name");
            let description = optional_field(form, "Description");
            let project = optional_field(form, "Project");
            let asset = application.add_project_asset(
                sfumato_core::application::AddProjectAssetCommand {
                    source: path,
                    name,
                    description,
                    alt_text: None,
                    tags: Vec::new(),
                    generation_prompt: None,
                    theme: None,
                    all_themes: false,
                    project,
                },
            )?;
            Ok(format!("Added project artifact '{}'", asset.name))
        }
        OperationKind::ArtifactRemove => {
            if !form.toggle("Confirm") {
                anyhow::bail!("Confirm removal before continuing");
            }
            let name = form.target.as_deref().context("Artifact name is missing")?;
            application.remove_project_asset(name, None)?;
            Ok(format!("Removed project artifact '{name}'"))
        }
        OperationKind::PromptCustomize(scope) => {
            if !form.toggle("Confirm") {
                anyhow::bail!("Confirm prompt customization before continuing");
            }
            let id = form
                .target
                .as_deref()
                .context("Prompt identifier is missing")?;
            let path = application.customize_prompt(id, scope, None)?;
            Ok(format!("Created prompt override at {}", path.display()))
        }
        OperationKind::PromptValidate => {
            let validation = application.validate_prompts(None)?;
            let mut summary = format!("Validated {} prompt templates", validation.resolved.len());
            // The TUI has no stderr, so an ignored override has to reach the
            // result line or it stays as invisible here as it was in the CLI.
            if !validation.unreferenced.is_empty() {
                summary.push_str(&format!(
                    "; {} ignored override file(s): {}",
                    validation.unreferenced.len(),
                    validation
                        .unreferenced
                        .iter()
                        .map(|stray| stray.path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Ok(summary)
        }
        OperationKind::ProjectEdit => {
            let project = form
                .target
                .clone()
                .context("Editing a project needs the project it belongs to")?;
            // Compared against what the project currently holds rather than written
            // unconditionally. `delete_config` reports an absent key as an error, so
            // clearing a picker that was already empty would abort the whole save; and
            // seven writes per visit would churn the file for nothing.
            let current = application.show_project(Some(&project))?;
            let wanted: [(&str, Option<&String>, Option<String>); 7] = [
                ("theme", Some(&current.theme), form.field("Theme")),
                (
                    "model_defaults.text",
                    current.model_defaults.get(&Capability::Text),
                    form.field("Text model"),
                ),
                (
                    "model_defaults.code",
                    current.model_defaults.get(&Capability::Code),
                    form.field("Code model"),
                ),
                (
                    "model_defaults.image",
                    current.model_defaults.get(&Capability::Image),
                    form.field("Image model"),
                ),
                (
                    "model_defaults.video",
                    current.model_defaults.get(&Capability::Video),
                    form.field("Video model"),
                ),
                (
                    "model_defaults.speech",
                    current.model_defaults.get(&Capability::Speech),
                    form.field("Speech model"),
                ),
                (
                    "model_roles.reviewer",
                    current.model_roles.get(&ModelRole::Reviewer),
                    form.field("Reviewer"),
                ),
            ];
            let mut written = Vec::new();
            for (key, present, value) in &wanted {
                // A field the form does not carry says nothing about the key, so it must
                // leave it alone. Reading it as an empty value would make an incomplete
                // form clear configuration it never showed the caller.
                let Some(value) = value else { continue };
                let present = present.filter(|existing| !existing.is_empty());
                match (present, value.as_str()) {
                    // A theme is required, so an empty picker there means "leave it",
                    // not "remove it" — there is no inherited theme to fall back to.
                    (_, "") if *key == "theme" => {}
                    (None, "") => {}
                    (Some(existing), "") => {
                        application.delete_config(
                            ConfigTarget::Project,
                            Some(project.clone()),
                            key,
                        )?;
                        written.push(format!("{key} cleared (was {existing})"));
                    }
                    (Some(existing), chosen) if existing == chosen => {}
                    (_, chosen) => {
                        application.set_config(
                            ConfigTarget::Project,
                            Some(project.clone()),
                            key,
                            chosen,
                        )?;
                        written.push(format!("{key} = {chosen}"));
                    }
                }
            }
            if written.is_empty() {
                Ok(format!("{project} was already up to date"))
            } else {
                Ok(format!("Updated {project}: {}", written.join(", ")))
            }
        }
        OperationKind::ToolSet(enabled) => {
            let name = form
                .target
                .clone()
                .context("Select a tool to enable or disable")?;
            let tool: GenerationToolKind = name
                .parse()
                .with_context(|| format!("'{name}' is not a generation tool"))?;
            application.set_generation_tool(tool, enabled, None)?;
            Ok(format!(
                "{name} is now {} for this project",
                if enabled { "enabled" } else { "disabled" }
            ))
        }
        OperationKind::PluginSet(enabled) => {
            let id = form
                .target
                .clone()
                .context("Select a plugin to enable or disable")?;
            let changed = if enabled {
                application.enable_page_plugin(&id, None)?
            } else {
                application.disable_page_plugin(&id, None)?
            };
            Ok(format!(
                "{id} is now {} for {}",
                if enabled { "enabled" } else { "disabled" },
                changed.project
            ))
        }
        OperationKind::ConfigSet => {
            let key = required_field(form, "Key")?;
            let path = application.set_config(
                config_target(&required_field(form, "Scope")?)?,
                optional_field(form, "Project"),
                &key,
                &required_field(form, "Value")?,
            )?;
            Ok(format!("Set {key} in {}", path.display()))
        }
        OperationKind::ConfigDelete => {
            if !form.toggle("Confirm deletion") {
                anyhow::bail!("Confirm deletion before continuing");
            }
            let key = required_field(form, "Key")?;
            let path = application.delete_config(
                config_target(&required_field(form, "Scope")?)?,
                optional_field(form, "Project"),
                &key,
            )?;
            Ok(format!("Deleted {key} from {}", path.display()))
        }
        OperationKind::SetupUser => {
            if application.user_config_exists() && !form.toggle("Overwrite existing config") {
                anyhow::bail!("User config already exists; confirm overwrite before continuing");
            }
            let preset = ConnectorPreset::from_str(&form.select("Connector"))?;
            let connector = preset.default_connector_name().to_string();
            let learning_style = split_values(&required_field(form, "Learning styles")?);
            if learning_style.is_empty() {
                anyhow::bail!("Learning styles must include at least one value");
            }
            let profile_name = required_field(form, "Profile")?;
            let connector_name = connector.clone();
            let mut config = GlobalConfig::default_config();
            config.user.name = Some(required_field(form, "Name")?);
            config.user.learning_style = learning_style;
            // `default_config` ships only a subset of the presets, and
            // `GlobalConfig::validate` rejects a profile naming an absent
            // connector, so configure the preset before referencing it.
            config
                .connectors
                .insert(connector.clone(), preset.into_config(&connector, None)?);
            config.models.insert(
                profile_name.clone(),
                ModelProfile {
                    connector,
                    model: required_field(form, "Model ID")?,
                    capabilities: preset.default_capabilities().to_vec(),
                    options: ModelOptions {
                        // Preset-derived rather than hardcoded: Anthropic rejects
                        // sampling parameters and shares `max_tokens` with
                        // thinking, so a 4000-token cap returns no text there.
                        text: TextModelOptions {
                            temperature: preset.default_text_temperature(),
                            max_tokens: preset.default_text_max_tokens(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                },
            );
            config.defaults = ModelDefaults(BTreeMap::from([(Capability::Text, profile_name)]));
            let result = application.setup_user(config)?;
            let login = if preset.requires_stored_login() {
                format!("; run `sfumato connector login {connector_name}` to store its API key")
            } else {
                String::new()
            };
            Ok(format!(
                "Initialized user config at {}{login}",
                result.path.display()
            ))
        }
    }
}

pub(super) fn load_section(
    section: Section,
    application: &SfumatoApplication,
) -> Result<Vec<BrowseRow>> {
    match section {
        Section::Projects => application
            .list_projects()?
            .into_iter()
            .map(|project| {
                let detail =
                    toml::to_string_pretty(&application.show_project(Some(&project.name))?)?;
                Ok(BrowseRow {
                    title: project.name,
                    subtitle: project.path.display().to_string(),
                    detail,
                    active: project.active,
                })
            })
            .collect(),
        Section::Models => application
            .list_models()?
            .into_iter()
            .map(|model| {
                let detail = toml::to_string_pretty(&application.show_model(&model.name)?)?;
                Ok(BrowseRow {
                    title: model.name,
                    subtitle: format!("{} / {}", model.connector, model.model),
                    detail,
                    active: false,
                })
            })
            .collect(),
        Section::Connectors => application
            .list_connectors()?
            .into_iter()
            .map(|connector| {
                let mut detail =
                    toml::to_string_pretty(&application.show_connector(&connector.name)?)?;
                let capabilities = application.connector_capabilities(&connector.name)?;
                detail.push_str("\nNative features:\n");
                if capabilities.features.is_empty() {
                    detail.push_str("- none\n");
                } else {
                    for feature in capabilities.features {
                        detail.push_str(&format!("- {}\n", feature.as_str()));
                    }
                }
                detail.push_str("\nCLI discovery: sfumato connector models ");
                detail.push_str(&connector.name);
                detail.push_str("\nCLI status: sfumato connector status ");
                detail.push_str(&connector.name);
                Ok(BrowseRow {
                    title: connector.name,
                    subtitle: format!("{} / {}", connector.kind, connector.target),
                    detail,
                    active: false,
                })
            })
            .collect(),
        Section::Themes => application
            .list_themes()?
            .into_iter()
            .map(|theme| {
                let package = application.show_theme(&theme.name)?;
                let detail = toml::to_string_pretty(&package.manifest)?;
                Ok(BrowseRow {
                    title: theme.name,
                    subtitle: "Reusable theme package".to_string(),
                    detail,
                    active: false,
                })
            })
            .collect(),
        Section::Templates => application
            .list_templates(None)?
            .entries
            .into_iter()
            .map(|template| {
                let package = application.show_template(&template.name, Some(template.kind))?;
                Ok(BrowseRow {
                    title: template.name,
                    subtitle: format!("{} template", template.kind),
                    detail: format!("{}\n\n{}", template.description, package.source),
                    active: false,
                })
            })
            .collect(),
        Section::Artifacts => application
            .list_project_assets(None)?
            .entries
            .into_iter()
            .map(|asset| {
                Ok(BrowseRow {
                    title: asset.name,
                    subtitle: asset
                        .variants
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", "),
                    detail: format!(
                        "{}\nSHA-256: {}\nFile: {}",
                        asset.metadata.description,
                        asset
                            .variants
                            .values()
                            .map(|variant| &variant.content_hash[..12])
                            .collect::<Vec<_>>()
                            .join(", "),
                        asset
                            .variants
                            .values()
                            .map(|variant| variant.path.display().to_string())
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                    active: false,
                })
            })
            .collect(),
        Section::Prompts => application
            .list_prompts(None)?
            .into_iter()
            .map(|template| {
                let source = application.show_prompt(template.id.as_str(), None)?;
                let provenance = template.provenance;
                let active = !matches!(provenance.origin, PromptOrigin::Bundled);
                let origin = match &provenance.origin {
                    PromptOrigin::Bundled => "bundled".to_string(),
                    PromptOrigin::User(path) => format!("user: {}", path.display()),
                    PromptOrigin::Project(path) => format!("project: {}", path.display()),
                };
                Ok(BrowseRow {
                    title: template.id.to_string(),
                    subtitle: origin,
                    detail: format!(
                        "SHA-256: {}\nVersion: {}\n\n{}",
                        provenance.content_hash, provenance.version, source.text
                    ),
                    active,
                })
            })
            .collect(),
        // The tool switches the CLI exposes as `sfumato tool`. Their status carries a
        // second fact beyond enabled/disabled: whether a model for the capability even
        // exists, because an enabled tool without one is silently absent at generation
        // time and that is invisible from the config alone.
        Section::Tools => Ok(application
            .list_generation_tools(None)?
            .into_iter()
            .map(|status| {
                let mut detail = format!(
                    "Tool: {}\nProject: {}\nModel for its capability: {}\n",
                    status.tool.as_str(),
                    if status.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    if status.model_configured {
                        "configured"
                    } else {
                        "missing"
                    },
                );
                if status.enabled && !status.model_configured {
                    detail.push_str(
                        "\nEnabled but unusable: no model profile provides the capability \
                         this tool needs, so the drafter is never offered it. Add one in \
                         Models, or set it as the project default in Projects > Edit.\n",
                    );
                }
                detail.push_str("\nCLI: sfumato tool enable ");
                detail.push_str(status.tool.as_str());
                BrowseRow {
                    title: status.tool.as_str().to_string(),
                    subtitle: match (status.enabled, status.model_configured) {
                        (true, true) => "enabled".to_string(),
                        (true, false) => "enabled, but no model provides it".to_string(),
                        (false, _) => "disabled".to_string(),
                    },
                    detail,
                    active: status.enabled,
                }
            })
            .collect()),
        // Installed plugins only. The full catalog listing reaches a remote registry,
        // which this synchronous load path cannot do; installing stays a CLI step.
        Section::Plugins => {
            let listing = application.list_installed_page_plugins()?;
            // The UI plugin lives in `page.ui`, not `page.plugins`, and the CLI treats
            // the union as enabled. Reading only the list showed the project's own UI
            // plugin as disabled.
            let (enabled, ui) = application
                .show_project(None)
                .map(|project| {
                    let ui = project.page.ui.clone();
                    let mut enabled = project.page.plugins;
                    enabled.extend(ui.clone());
                    (enabled, ui)
                })
                .unwrap_or_default();
            let mut rows = listing
                .entries
                .into_iter()
                .map(|plugin| {
                    let on = enabled.contains(&plugin.id);
                    let is_ui = ui.as_deref() == Some(plugin.id.as_str());
                    // Kept short: the row is one line beside a name and a version, and
                    // the full explanation belongs in the detail pane.
                    let state = match (on, is_ui) {
                        (true, true) => "enabled (UI)",
                        (true, false) => "enabled",
                        (false, _) => "disabled",
                    };
                    let mut detail = format!(
                        "Plugin: {}\nVersion: {}\nBrowser global: {}\nCategory: {}\n\
                         Project: {}\nLicense: {}\n",
                        plugin.name,
                        plugin.version,
                        plugin.api_global,
                        format!("{:?}", plugin.category).to_lowercase(),
                        if is_ui && on {
                            "enabled as this project's UI plugin"
                        } else {
                            state
                        },
                        plugin.license,
                    );
                    // Switching one of these fails at the layer below, so saying it here
                    // beats offering an action that cannot work.
                    if plugin.category == PagePluginCategory::Runtime {
                        detail.push_str(
                            "\nManaged as a dependency: a runtime plugin is selected by \
                             whatever needs it and cannot be switched directly.\n",
                        );
                    }
                    detail.push_str(&format!(
                        "\nCLI: sfumato plugin enable {}\n\
                         Install or update others with: sfumato plugin install <id>\n",
                        plugin.id,
                    ));
                    BrowseRow {
                        subtitle: format!("{} {} · {state}", plugin.name, plugin.version),
                        title: plugin.id,
                        detail,
                        active: on,
                    }
                })
                .collect::<Vec<_>>();
            // An unreadable package is a fact about the store, not something to hide
            // behind a shorter list.
            rows.extend(listing.unreadable.into_iter().map(|entry| BrowseRow {
                subtitle: "cannot be read".to_string(),
                detail: entry.problem.clone(),
                title: entry.name,
                active: false,
            }));
            Ok(rows)
        }
        Section::Configuration => {
            let rows = [
                ("Effective", ConfigTarget::Effective),
                ("User", ConfigTarget::User),
                ("Project", ConfigTarget::Project),
            ];
            rows.into_iter()
                .map(|(name, target)| {
                    let detail = application
                        .show_config(target, None, None)
                        .unwrap_or_else(|error| format!("{error:#}"));
                    Ok(BrowseRow {
                        title: name.to_string(),
                        subtitle: "TOML configuration".to_string(),
                        detail,
                        active: name == "Effective",
                    })
                })
                .collect()
        }
        Section::Setup => {
            let state = if application.user_config_exists() {
                "Configured"
            } else {
                "Not configured"
            };
            Ok(vec![BrowseRow {
                title: "User configuration".to_string(),
                subtitle: state.to_string(),
                detail: format!(
                    "Status: {state}\nPath: {}\n\nThe user profile owns learning preferences, connectors, model profiles, and user defaults.",
                    application.user_config_path().display()
                ),
                active: application.user_config_exists(),
            }])
        }
    }
}
