//! Runtime-neutral TUI state for one active resource operation.

use super::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Projects,
    Models,
    Connectors,
    Themes,
    Templates,
    Artifacts,
    Prompts,
    Tools,
    Plugins,
    Configuration,
    Setup,
}

impl Section {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Models => "Models",
            Self::Connectors => "Connectors",
            Self::Themes => "Themes",
            Self::Templates => "Templates",
            Self::Artifacts => "Artifacts",
            Self::Tools => "Tools",
            Self::Plugins => "Plugins",
            Self::Prompts => "Prompts",
            Self::Configuration => "Configuration",
            Self::Setup => "Setup",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Screen {
    Home,
    Browse(Section),
    Generate,
    Edit,
    Running,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResourceOperation {
    Generate,
    GenerateDocument,
    GeneratePage,
    GenerateVideo,
    Edit,
}

#[derive(Clone, Debug)]
pub(super) struct BrowseRow {
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) detail: String,
    pub(super) active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActivityKind {
    Stage,
    Model,
    ToolCall,
    ToolResult,
    Warning,
    Success,
    /// A committed artifact and where it landed.
    ///
    /// Its own kind because it is the one entry that must survive being long: a
    /// managed revision path runs past a hundred characters, and an entry that
    /// answers "where is it" is worthless clipped.
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BrowseFocus {
    Actions,
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BrowseAction {
    ProjectCreate,
    ProjectActivate,
    ProjectEdit,
    ProjectRemove,
    ModelAdd,
    ModelEdit,
    ModelUse,
    ModelRemove,
    ConnectorSetup,
    ConnectorModels,
    ConnectorStatus,
    ThemeCreate,
    ThemeImport,
    ThemeExport,
    ThemeUse,
    TemplateCreate,
    ArtifactAdd,
    ArtifactRemove,
    ToolEnable,
    ToolDisable,
    PluginEnable,
    PluginDisable,
    PromptCustomizeUser,
    PromptCustomizeProject,
    PromptValidate,
    ConfigSet,
    ConfigDelete,
    SetupUser,
}

impl BrowseAction {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ProjectCreate => "Create",
            Self::ProjectActivate => "Activate",
            Self::ProjectEdit => "Edit",
            Self::ProjectRemove => "Remove",
            Self::ModelAdd => "Add",
            Self::ModelEdit => "Edit",
            Self::ModelUse => "Set default",
            Self::ModelRemove => "Remove",
            Self::ConnectorSetup => "Setup",
            Self::ConnectorModels => "Model catalog",
            Self::ConnectorStatus => "Native status",
            Self::ThemeCreate => "Create",
            Self::ThemeImport => "Import DESIGN.md",
            Self::ThemeExport => "Export DESIGN.md",
            Self::ThemeUse => "Apply",
            Self::TemplateCreate => "Create",
            Self::ArtifactAdd => "Add",
            Self::ArtifactRemove => "Remove",
            Self::ToolEnable => "Enable",
            Self::ToolDisable => "Disable",
            Self::PluginEnable => "Enable",
            Self::PluginDisable => "Disable",
            Self::PromptCustomizeUser => "Customize user",
            Self::PromptCustomizeProject => "Customize project",
            Self::PromptValidate => "Validate",
            Self::ConfigSet => "Set value",
            Self::ConfigDelete => "Delete value",
            Self::SetupUser => "Initialize user",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OperationKind {
    ProjectCreate,
    ProjectEdit,
    ProjectRemove,
    ToolSet(bool),
    PluginSet(bool),
    ModelAdd,
    ModelEdit,
    ModelUse,
    ModelRemove,
    ConnectorSetup,
    ThemeCreate,
    ThemeImport,
    ThemeExport,
    ThemeUse,
    TemplateCreate,
    ArtifactAdd,
    ArtifactRemove,
    PromptCustomize(PromptOverrideScope),
    PromptValidate,
    ConfigSet,
    ConfigDelete,
    SetupUser,
}

/// Label of the optional credential field on the connector setup form.
///
/// Shared so the form definition and the preset-dependency pass cannot drift.
pub(super) const API_KEY_ENV_FIELD: &str = "API key environment";

#[derive(Clone, Debug)]
pub(super) struct OperationForm {
    pub(super) title: &'static str,
    pub(super) kind: OperationKind,
    pub(super) target: Option<String>,
    pub(super) fields: Vec<FormField>,
    pub(super) selected: usize,
}

impl OperationForm {
    pub(super) fn text(&self, label: &str) -> String {
        self.fields
            .iter()
            .find_map(|field| match field {
                // A picked value reads the same as a typed one, so every effect that
                // already reads a field by label keeps working when it becomes a picker.
                FormField::Text {
                    label: field_label,
                    value,
                    ..
                }
                | FormField::Choice {
                    label: field_label,
                    value,
                    ..
                } if *field_label == label => Some(value.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The value of a field, or `None` when the form has no such field.
    ///
    /// [`Self::text`] cannot tell an emptied field from an absent one, and for anything
    /// that treats empty as "remove this" the difference decides whether a missing field
    /// silently deletes configuration.
    pub(super) fn field(&self, label: &str) -> Option<String> {
        self.fields.iter().find_map(|field| match field {
            FormField::Text {
                label: field_label,
                value,
                ..
            }
            | FormField::Choice {
                label: field_label,
                value,
                ..
            } if *field_label == label => Some(value.trim().to_string()),
            _ => None,
        })
    }

    pub(super) fn toggle(&self, label: &str) -> bool {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Toggle {
                    label: field_label,
                    value,
                } if *field_label == label => Some(*value),
                _ => None,
            })
            .unwrap_or(false)
    }

    /// Re-derives the fields that depend on a select choice.
    ///
    /// Called after every select move so the form never offers a field the
    /// chosen preset rejects, and never keeps another preset's defaults.
    pub(super) fn apply_select_dependencies(&mut self) {
        match self.kind {
            OperationKind::ConnectorSetup => {
                let accepts = ConnectorPreset::from_str(&self.select("Preset"))
                    .map(ConnectorPreset::accepts_api_key_env)
                    // An unparsable value is rejected on submit; keep the field.
                    .unwrap_or(true);
                self.set_field_present(API_KEY_ENV_FIELD, accepts, || FormField::Text {
                    label: API_KEY_ENV_FIELD,
                    value: String::new(),
                    placeholder: "optional CI environment variable",
                    multiline: false,
                });
            }
            OperationKind::SetupUser => {
                let Ok(preset) = ConnectorPreset::from_str(&self.select("Connector")) else {
                    return;
                };
                // Overwritten rather than merged: these two fields are the chosen
                // preset's defaults, and a stale Ollama profile name or model id
                // would be written into the config as-is.
                self.set_text("Profile", preset.default_profile_name());
                self.set_text("Model ID", preset.default_model());
            }
            _ => {}
        }
    }

    fn set_text(&mut self, label: &str, value: &str) {
        for field in &mut self.fields {
            if let FormField::Text {
                label: field_label,
                value: field_value,
                ..
            } = field
                && *field_label == label
            {
                *field_value = value.to_string();
            }
        }
    }

    fn set_field_present(
        &mut self,
        label: &'static str,
        present: bool,
        build: impl FnOnce() -> FormField,
    ) {
        let position = self.fields.iter().position(|field| field.label() == label);
        match (present, position) {
            (true, None) => {
                let insert_at = self
                    .fields
                    .iter()
                    .position(|field| matches!(field, FormField::Submit { .. }))
                    .unwrap_or(self.fields.len());
                self.fields.insert(insert_at, build());
                if self.selected >= insert_at {
                    self.selected += 1;
                }
            }
            (false, Some(index)) => {
                self.fields.remove(index);
                if self.selected > index {
                    self.selected -= 1;
                }
                self.selected = self.selected.min(self.fields.len().saturating_sub(1));
            }
            _ => {}
        }
    }

    pub(super) fn select(&self, label: &str) -> String {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Select {
                    label: field_label,
                    options,
                    selected,
                } if *field_label == label => options.get(*selected).cloned(),
                _ => None,
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub(super) struct Activity {
    pub(super) kind: ActivityKind,
    pub(super) title: String,
    pub(super) detail: String,
    pub(super) image_path: Option<PathBuf>,
}

impl Activity {
    pub(super) fn from_event(event: &TextGenerationEvent) -> Self {
        match event {
            TextGenerationEvent::StageStarted { stage, profile } => Self {
                kind: ActivityKind::Stage,
                title: stage.as_str().to_string(),
                detail: profile
                    .as_ref()
                    .map(|profile| format!("model profile: {profile}"))
                    .unwrap_or_default(),
                image_path: None,
            },
            TextGenerationEvent::RequestStarted { round } => Self {
                kind: ActivityKind::Model,
                title: format!("Model request {round}"),
                detail: "Waiting for a response".to_string(),
                image_path: None,
            },
            TextGenerationEvent::ModelSelected {
                model,
                display_name,
            } => Self {
                kind: ActivityKind::Model,
                title: format!("Selected {display_name}"),
                detail: model.clone(),
                image_path: None,
            },
            TextGenerationEvent::ToolCallRequested { name, arguments } => Self {
                kind: ActivityKind::ToolCall,
                title: tool_name(name),
                detail: format_tool_arguments(arguments),
                image_path: None,
            },
            TextGenerationEvent::ToolCallSucceeded { name, result } => {
                let (detail, image_path) = format_tool_result(name, result);
                Self {
                    kind: ActivityKind::ToolResult,
                    title: format!("{} complete", tool_name(name)),
                    detail,
                    image_path,
                }
            }
            TextGenerationEvent::ToolCallFailed { name, error } => Self {
                kind: ActivityKind::Warning,
                title: format!("{} failed", tool_name(name)),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::ResponseCompleted => Self {
                kind: ActivityKind::Success,
                title: "Model response complete".to_string(),
                detail: String::new(),
                image_path: None,
            },
            TextGenerationEvent::DraftTitleRepairStarted { error } => Self {
                kind: ActivityKind::Warning,
                title: "Repairing deck title".to_string(),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::ReviewRetryStarted { attempt, error } => Self {
                kind: ActivityKind::Warning,
                title: format!("Content review retry {attempt}"),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::ContextCompactionStarted {
                stage,
                original_chars,
                compacted_chars,
            } => Self {
                kind: ActivityKind::Warning,
                title: "Compacting model context".to_string(),
                detail: format!(
                    "{} reduced from {original_chars} to {compacted_chars} characters",
                    stage.as_str()
                ),
                image_path: None,
            },
            TextGenerationEvent::LayoutCheckCompleted { issues } => Self {
                kind: if *issues == 0 {
                    ActivityKind::Success
                } else {
                    ActivityKind::Warning
                },
                title: if *issues == 0 {
                    "Layout check passed".to_string()
                } else {
                    format!("{issues} slide(s) need repair")
                },
                detail: String::new(),
                image_path: None,
            },
            TextGenerationEvent::LayoutSlideRepairStarted {
                slide,
                position,
                total,
                profile,
            } => Self {
                kind: ActivityKind::ToolCall,
                title: format!("Repairing slide {slide}"),
                detail: format!("{position} of {total} with {profile}"),
                image_path: None,
            },
            TextGenerationEvent::LayoutSlideRepairRetryStarted {
                slide,
                attempt,
                error,
            } => Self {
                kind: ActivityKind::Warning,
                title: format!("Slide {slide} repair retry {attempt}"),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::SourceRepairStarted { reason, scene } => Self {
                kind: ActivityKind::Warning,
                title: match scene {
                    Some(scene) => format!("Repairing {scene}"),
                    None => "Repairing source".to_string(),
                },
                detail: compact(reason, 300),
                image_path: None,
            },
        }
    }

    pub(super) fn from_operation_event(event: &OperationEvent) -> Option<Self> {
        if event.kind == OperationEventKind::Progress {
            return None;
        }
        let title = operation_stage_label(event.stage).to_string();
        let detail = event
            .fields
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(Self {
            kind: match event.kind {
                OperationEventKind::Completed => ActivityKind::Success,
                OperationEventKind::Warning | OperationEventKind::Retry => ActivityKind::Warning,
                OperationEventKind::Started | OperationEventKind::Progress => ActivityKind::Model,
                _ => ActivityKind::Model,
            },
            title,
            detail,
            image_path: None,
        })
    }
}

pub(super) fn operation_stage_label(stage: OperationStage) -> &'static str {
    match stage {
        OperationStage::Resolve => "Resolving configuration",
        OperationStage::ReadSources => "Reading source material",
        OperationStage::RenderPrompt => "Rendering prompts",
        OperationStage::Draft => "Drafting resource",
        OperationStage::Edit => "Editing resource",
        OperationStage::Review => "Reviewing content",
        OperationStage::InspectLayout => "Inspecting layout",
        OperationStage::Repair => "Repairing resource",
        OperationStage::Render => "Rendering artifacts",
        OperationStage::CommitArtifacts => "Committing revision",
        OperationStage::Publish => "Publishing output",
        // `OperationStage` is `#[non_exhaustive]`, so a downstream match cannot be
        // exhaustive and a new variant cannot produce a compile error here — unlike
        // `stage_label` in `view.rs`, which covers `GenerationStage` from this crate.
        // Falling back to the stage's own stable name means an unlabelled stage still
        // says which one it is, instead of every one of them reading as the same
        // opaque "Running operation" in the activity list.
        other => other.as_str(),
    }
}

pub(super) enum UiMessage {
    GenerationEvent {
        job_id: u64,
        event: TextGenerationEvent,
    },
    OperationEvent {
        job_id: u64,
        event: OperationEvent,
    },
    ResourceFinished {
        job_id: u64,
        result: Box<Result<ResourceResult, String>>,
    },
    ResourceCancelled {
        job_id: u64,
    },
    ConnectorQueryFinished {
        connector: String,
        result: Result<Vec<BrowseRow>, String>,
    },
}

pub(super) enum ResourceResult {
    Generated(GenerateSlidesResult),
    GeneratedDocument(GenerateDocumentResult),
    GeneratedPage(GeneratePageResult),
    GeneratedVideo(GenerateVideoResult),
    Edited(EditSlidesResult),
}

impl ResourceResult {
    pub(super) fn markdown_path(&self) -> &std::path::Path {
        match self {
            Self::Generated(result) => &result.markdown_path,
            Self::GeneratedDocument(result) => &result.markdown_path,
            Self::GeneratedPage(result) => &result.html_path,
            Self::GeneratedVideo(result) => &result.video_path,
            Self::Edited(result) => &result.markdown_path,
        }
    }

    pub(super) fn warnings(&self) -> &[String] {
        match self {
            Self::Generated(result) => &result.warnings,
            Self::GeneratedDocument(result) => &result.warnings,
            Self::GeneratedPage(result) => &result.warnings,
            Self::GeneratedVideo(result) => &result.output.warnings,
            Self::Edited(result) => &result.warnings,
        }
    }

    /// Every committed artifact, labelled, in the order worth reading.
    ///
    /// The published copy comes first when there is one: it is the path the caller
    /// asked for, and the managed revision is where it lives regardless.
    pub(super) fn artifacts(&self) -> Vec<(&'static str, PathBuf)> {
        let mut artifacts = Vec::new();
        match self {
            Self::Generated(result) => {
                if let Some(path) = &result.published_pdf_path {
                    artifacts.push(("Published PDF", path.clone()));
                }
                if let Some(path) = &result.pdf_path {
                    artifacts.push(("PDF", path.clone()));
                }
                artifacts.push(("Markdown", result.markdown_path.clone()));
            }
            Self::GeneratedDocument(result) => {
                if let Some(path) = &result.published_pdf_path {
                    artifacts.push(("Published PDF", path.clone()));
                }
                if let Some(path) = &result.pdf_path {
                    artifacts.push(("PDF", path.clone()));
                }
                artifacts.push(("Markdown", result.markdown_path.clone()));
            }
            Self::GeneratedPage(result) => {
                artifacts.extend(
                    result
                        .published_paths
                        .iter()
                        .map(|path| ("Published page", path.clone())),
                );
                artifacts.push(("Page", result.html_path.clone()));
            }
            Self::GeneratedVideo(result) => {
                artifacts.extend(
                    result
                        .published_paths
                        .iter()
                        .map(|path| ("Published video", path.clone())),
                );
                artifacts.push(("Video", result.video_path.clone()));
            }
            Self::Edited(result) => artifacts.push(("Markdown", result.markdown_path.clone())),
        }
        artifacts
    }

    pub(super) fn completion_message(&self) -> &'static str {
        match self {
            Self::Generated(_) => "Generation complete",
            Self::GeneratedDocument(_) => "Document generation complete",
            Self::GeneratedPage(_) => "Page generation complete",
            Self::GeneratedVideo(_) => "Video generation complete",
            Self::Edited(_) => "Slide edit complete",
        }
    }
}

pub(super) struct App {
    pub(super) application: Arc<SfumatoApplication>,
    pub(super) screen: Screen,
    pub(super) nav_index: usize,
    pub(super) browse_rows: Vec<BrowseRow>,
    pub(super) browse_index: usize,
    pub(super) browse_focus: BrowseFocus,
    pub(super) browse_action_index: usize,
    pub(super) browse_detail_scroll: u16,
    pub(super) connector_query_source: Option<String>,
    pub(super) operation: Option<OperationForm>,
    pub(super) form: GenerateForm,
    pub(super) edit_form: EditForm,
    pub(super) resource_operation: ResourceOperation,
    pub(super) activities: Vec<Activity>,
    pub(super) activity_index: usize,
    pub(super) current_stage: Option<GenerationStage>,
    pub(super) generation_failed: bool,
    pub(super) result: Option<ResourceResult>,
    pub(super) status: Option<(String, bool)>,
    pub(super) tick: usize,
    pub(super) should_quit: bool,
    pub(super) sender: Sender<UiMessage>,
    pub(super) messages: Receiver<UiMessage>,
    pub(super) jobs: OperationLifecycle,
    pub(super) active_task: Option<JoinHandle<()>>,
    /// In-flight native connector read, kept so it can be cancelled.
    ///
    /// Separate from `jobs`, which owns the generation job: browsing a connector
    /// must not cancel a running generation, and `Esc` in the browse view must
    /// not have to wait on one.
    pub(super) connector_query: Option<ConnectorQuery>,
    /// Workspace state the chrome and home screen render from.
    ///
    /// Refreshed by `refresh_snapshot`, never during a draw: the values only change
    /// when this process performs an action, and reading them per frame made the
    /// render loop do filesystem work.
    pub(super) snapshot: WorkspaceSnapshot,
    /// When the running operation started, for the elapsed clock.
    ///
    /// A long video render can run for minutes; without a clock there is no way to
    /// tell a slow stage from a stuck one.
    pub(super) started_at: Option<std::time::Instant>,
    /// Palette or help overlay, when one is open.
    ///
    /// Drawn over whatever screen is underneath and consuming keys while open, so a
    /// jump never loses the state of the screen it was launched from.
    pub(super) overlay: Option<Overlay>,
    pub(super) picker: Picker,
    pub(super) image: Option<StatefulProtocol>,
    pub(super) effects: EffectManager<&'static str>,
    pub(super) dirty: bool,
}

#[cfg(test)]
#[path = "../../tests/unit/tui_model.rs"]
mod tests;
