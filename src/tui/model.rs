//! Runtime-neutral TUI state for one active resource operation.

use super::*;
use std::sync::Arc;

use sfumato_core::operation::{CancellationHandle, EventSink, OperationContext};

/// Owns UI job identity and the matching core cancellation handle.
pub(super) struct OperationLifecycle {
    next_job_id: u64,
    active_job_id: Option<u64>,
    cancellation: Option<CancellationHandle>,
}

impl Default for OperationLifecycle {
    fn default() -> Self {
        Self {
            next_job_id: 1,
            active_job_id: None,
            cancellation: None,
        }
    }
}

impl OperationLifecycle {
    pub(super) fn next_job_id(&self) -> u64 {
        self.next_job_id
    }

    pub(super) fn begin(&mut self, events: Arc<dyn EventSink>) -> (u64, OperationContext) {
        self.cancel();
        let job_id = self.next_job_id;
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        let (cancellation, operation) = OperationContext::create(None, events);
        self.active_job_id = Some(job_id);
        self.cancellation = Some(cancellation);
        (job_id, operation)
    }

    pub(super) fn is_active(&self, job_id: u64) -> bool {
        self.active_job_id == Some(job_id)
    }

    pub(super) fn finish(&mut self, job_id: u64) -> bool {
        if !self.is_active(job_id) {
            return false;
        }
        self.active_job_id = None;
        self.cancellation = None;
        true
    }

    pub(super) fn cancel(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.cancel();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Projects,
    Models,
    Connectors,
    Themes,
    Templates,
    Artifacts,
    Prompts,
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
    GeneratePage,
    Edit,
}

#[derive(Clone, Debug)]
pub(super) struct BrowseRow {
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) detail: String,
    pub(super) active: bool,
}

#[derive(Clone, Debug)]
pub(super) enum FormField {
    Text {
        label: &'static str,
        value: String,
        placeholder: &'static str,
        multiline: bool,
    },
    Toggle {
        label: &'static str,
        value: bool,
    },
    Select {
        label: &'static str,
        options: Vec<String>,
        selected: usize,
    },
    MultiSelect {
        label: &'static str,
        options: Vec<String>,
        cursor: usize,
        selected: BTreeSet<usize>,
    },
    Submit {
        label: &'static str,
    },
}

impl FormField {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Text { label, .. }
            | Self::Toggle { label, .. }
            | Self::Select { label, .. }
            | Self::MultiSelect { label, .. } => label,
            Self::Submit { label } => label,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct GenerateForm {
    pub(super) fields: Vec<FormField>,
    pub(super) selected: usize,
}

#[derive(Clone, Debug)]
pub(super) struct EditForm {
    pub(super) fields: Vec<FormField>,
    pub(super) selected: usize,
}

impl Default for EditForm {
    fn default() -> Self {
        Self {
            fields: vec![
                FormField::Text {
                    label: "Deck",
                    value: String::new(),
                    placeholder: "~/.sfumato/Projects/university/slides/deck.md",
                    multiline: false,
                },
                FormField::Text {
                    label: "Instruction",
                    value: String::new(),
                    placeholder: "Clarify the explanation on slide four",
                    multiline: true,
                },
                FormField::Text {
                    label: "Project",
                    value: String::new(),
                    placeholder: "active project",
                    multiline: false,
                },
                FormField::Text {
                    label: "Text model",
                    value: String::new(),
                    placeholder: "project or user default",
                    multiline: false,
                },
                FormField::Submit {
                    label: "Edit slides",
                },
            ],
            selected: 0,
        }
    }
}

impl EditForm {
    pub(super) fn text(&self, label: &str) -> String {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Text {
                    label: field_label,
                    value,
                    ..
                } if *field_label == label => Some(value.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub(super) fn to_args(&self) -> Result<EditSlidesArgs> {
        let markdown_path = self.text("Deck");
        if markdown_path.is_empty() {
            anyhow::bail!("Deck path cannot be empty");
        }
        let instruction = self.text("Instruction");
        if instruction.is_empty() {
            anyhow::bail!("Instruction cannot be empty");
        }
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let text_model = self.text("Text model");
        Ok(EditSlidesArgs {
            markdown_path: PathBuf::from(markdown_path),
            instruction,
            project: optional(self.text("Project")),
            model_overrides: if text_model.is_empty() {
                Vec::new()
            } else {
                vec![format!("text={text_model}")]
            },
            json: false,
        })
    }
}

impl Default for GenerateForm {
    fn default() -> Self {
        Self::with_plugins(Vec::new())
    }
}

impl GenerateForm {
    pub(super) fn with_plugins(plugins: Vec<String>) -> Self {
        Self {
            fields: vec![
                FormField::Select {
                    label: "Resource",
                    options: vec!["Slides".into(), "Page".into()],
                    selected: 0,
                },
                FormField::Text {
                    label: "Instruction",
                    value: String::new(),
                    placeholder: "Explain Fourier series visually",
                    multiline: true,
                },
                FormField::Text {
                    label: "Sources",
                    value: String::new(),
                    placeholder: "notes, course-material",
                    multiline: false,
                },
                FormField::Text {
                    label: "Project",
                    value: String::new(),
                    placeholder: "active project",
                    multiline: false,
                },
                FormField::Text {
                    label: "Title",
                    value: String::new(),
                    placeholder: "generated by the drafter",
                    multiline: false,
                },
                FormField::Text {
                    label: "Theme",
                    value: String::new(),
                    placeholder: "project theme",
                    multiline: false,
                },
                FormField::Text {
                    label: "Template",
                    value: String::new(),
                    placeholder: "optional reusable structure",
                    multiline: false,
                },
                FormField::Text {
                    label: "Publish PDF",
                    value: String::new(),
                    placeholder: "optional folder",
                    multiline: false,
                },
                FormField::Text {
                    label: "Text model",
                    value: String::new(),
                    placeholder: "project or user default",
                    multiline: false,
                },
                FormField::Text {
                    label: "Reviewer",
                    value: String::new(),
                    placeholder: "project or user reviewer",
                    multiline: false,
                },
                FormField::MultiSelect {
                    label: "Page plugins",
                    options: plugins,
                    cursor: 0,
                    selected: BTreeSet::new(),
                },
                FormField::Toggle {
                    label: "Review",
                    value: true,
                },
                FormField::Toggle {
                    label: "Dry run",
                    value: false,
                },
                FormField::Submit {
                    label: "Generate slides",
                },
            ],
            selected: 0,
        }
    }
}

impl GenerateForm {
    pub(super) fn is_page(&self) -> bool {
        self.fields.iter().any(|field| {
            matches!(
                field,
                FormField::Select {
                    label: "Resource",
                    selected: 1,
                    ..
                }
            )
        })
    }

    pub(super) fn selected_plugins(&self) -> Vec<String> {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::MultiSelect {
                    label: "Page plugins",
                    options,
                    selected,
                    ..
                } => Some(
                    selected
                        .iter()
                        .filter_map(|index| options.get(*index).cloned())
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub(super) fn text(&self, label: &str) -> String {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Text {
                    label: field_label,
                    value,
                    ..
                } if *field_label == label => Some(value.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default()
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

    pub(super) fn to_args(&self) -> Result<SlidesArgs> {
        let instruction = self.text("Instruction");
        if instruction.is_empty() {
            anyhow::bail!("Instruction cannot be empty");
        }
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let inputs = split_values(&self.text("Sources"))
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let text_model = self.text("Text model");
        Ok(SlidesArgs {
            inputs,
            instruction,
            title: optional(self.text("Title")),
            template: optional(self.text("Template")),
            out: optional(self.text("Publish PDF")).map(PathBuf::from),
            pdf: true,
            dry_run: self.toggle("Dry run"),
            project: optional(self.text("Project")),
            theme: optional(self.text("Theme")),
            model_overrides: if text_model.is_empty() {
                Vec::new()
            } else {
                vec![format!("text={text_model}")]
            },
            review_model: optional(self.text("Reviewer")),
            no_review: !self.toggle("Review"),
            json: false,
        })
    }

    pub(super) fn to_page_args(&self) -> Result<PageArgs> {
        let instruction = self.text("Instruction");
        if instruction.is_empty() {
            anyhow::bail!("Instruction cannot be empty");
        }
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let inputs = split_values(&self.text("Sources"))
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let text_model = self.text("Text model");
        Ok(PageArgs {
            inputs,
            instruction,
            title: optional(self.text("Title")),
            template: optional(self.text("Template")),
            out: optional(self.text("Publish PDF")).map(PathBuf::from),
            dry_run: self.toggle("Dry run"),
            project: optional(self.text("Project")),
            theme: optional(self.text("Theme")),
            model_overrides: if text_model.is_empty() {
                Vec::new()
            } else {
                vec![format!("text={text_model}")]
            },
            review_model: optional(self.text("Reviewer")),
            plugins: self.selected_plugins(),
            shadcn: false,
            no_review: !self.toggle("Review"),
            json: false,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActivityKind {
    Stage,
    Model,
    ToolCall,
    ToolResult,
    Warning,
    Success,
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
    ProjectRemove,
    ModelAdd,
    ModelEdit,
    ModelUse,
    ModelRemove,
    ConnectorOllama,
    ConnectorOpenrouter,
    ThemeCreate,
    ThemeImport,
    ThemeExport,
    ThemeUse,
    TemplateCreate,
    ArtifactAdd,
    ArtifactRemove,
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
            Self::ProjectRemove => "Remove",
            Self::ModelAdd => "Add",
            Self::ModelEdit => "Edit",
            Self::ModelUse => "Set default",
            Self::ModelRemove => "Remove",
            Self::ConnectorOllama => "Setup Ollama",
            Self::ConnectorOpenrouter => "Setup OpenRouter",
            Self::ThemeCreate => "Create",
            Self::ThemeImport => "Import DESIGN.md",
            Self::ThemeExport => "Export DESIGN.md",
            Self::ThemeUse => "Apply",
            Self::TemplateCreate => "Create",
            Self::ArtifactAdd => "Add",
            Self::ArtifactRemove => "Remove",
            Self::PromptCustomizeUser => "Customize user",
            Self::PromptCustomizeProject => "Customize project",
            Self::PromptValidate => "Validate",
            Self::ConfigSet => "Set value",
            Self::ConfigDelete => "Delete value",
            Self::SetupUser => "Initialize user",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum OperationKind {
    ProjectCreate,
    ProjectRemove,
    ModelAdd,
    ModelEdit,
    ModelUse,
    ModelRemove,
    ConnectorSetup(ConnectorPreset),
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
                FormField::Text {
                    label: field_label,
                    value,
                    ..
                } if *field_label == label => Some(value.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default()
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
        _ => "Running operation",
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
}

pub(super) enum ResourceResult {
    Generated(GenerateSlidesResult),
    GeneratedPage(GeneratePageResult),
    Edited(EditSlidesResult),
}

impl ResourceResult {
    pub(super) fn markdown_path(&self) -> &std::path::Path {
        match self {
            Self::Generated(result) => &result.markdown_path,
            Self::GeneratedPage(result) => &result.html_path,
            Self::Edited(result) => &result.markdown_path,
        }
    }

    pub(super) fn warnings(&self) -> &[String] {
        match self {
            Self::Generated(result) => &result.warnings,
            Self::GeneratedPage(result) => &result.warnings,
            Self::Edited(result) => &result.warnings,
        }
    }

    pub(super) fn completion_message(&self) -> &'static str {
        match self {
            Self::Generated(_) => "Generation complete",
            Self::GeneratedPage(_) => "Page generation complete",
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
    pub(super) picker: Picker,
    pub(super) image: Option<StatefulProtocol>,
    pub(super) effects: EffectManager<&'static str>,
    pub(super) dirty: bool,
}

#[cfg(test)]
#[path = "../../tests/unit/tui_model.rs"]
mod tests;
