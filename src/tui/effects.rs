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
        OperationKind::ConnectorSetup(preset) => {
            let connector = application.setup_connector(
                preset,
                optional_field(form, "Name"),
                required_field(form, "API key environment")?,
            )?;
            Ok(format!("Configured connector '{}'", connector.name))
        }
        OperationKind::ThemeCreate => {
            let name = required_field(form, "Name")?;
            application.create_theme(&name)?;
            Ok(format!("Created theme '{name}'"))
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
        OperationKind::PromptCustomize(scope) => {
            if !form.toggle("Confirm") {
                anyhow::bail!("Confirm prompt customization before continuing");
            }
            let id = PromptId::from_str(
                form.target
                    .as_deref()
                    .context("Prompt identifier is missing")?,
            )?;
            let path = application.customize_prompt(id, scope, None)?;
            Ok(format!("Created prompt override at {}", path.display()))
        }
        OperationKind::PromptValidate => {
            let prompts = application.validate_prompts(None)?;
            Ok(format!("Validated {} prompt templates", prompts.len()))
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
            let connector = required_field(form, "Connector")?;
            if connector != "ollama" && connector != "openrouter" {
                anyhow::bail!("Connector must be 'ollama' or 'openrouter'");
            }
            let learning_style = split_values(&required_field(form, "Learning styles")?);
            if learning_style.is_empty() {
                anyhow::bail!("Learning styles must include at least one value");
            }
            let profile_name = required_field(form, "Profile")?;
            let mut config = GlobalConfig::default_config();
            config.user.name = Some(required_field(form, "Name")?);
            config.user.learning_style = learning_style;
            config.models.insert(
                profile_name.clone(),
                ModelProfile {
                    connector,
                    model: required_field(form, "Model ID")?,
                    capabilities: vec![Capability::Text, Capability::Code],
                    options: ModelOptions {
                        text: TextModelOptions {
                            temperature: Some(0.4),
                            max_tokens: Some(4000),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                },
            );
            config.defaults = ModelDefaults(BTreeMap::from([(Capability::Text, profile_name)]));
            config
                .connectors
                .get_mut("openrouter")
                .context("Default OpenRouter connector is missing")?
                .credential = Some(SecretRef::environment(&required_field(
                form,
                "API key environment",
            )?)?);
            let result = application.setup_user(config)?;
            Ok(format!(
                "Initialized user config at {}",
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
                let detail = toml::to_string_pretty(&application.show_connector(&connector.name)?)?;
                Ok(BrowseRow {
                    title: connector.name,
                    subtitle: connector.base_url,
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
        Section::Prompts => application
            .list_prompts(None)?
            .into_iter()
            .map(|template| {
                let source = application.show_prompt(template.id, None)?;
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
