use std::{collections::BTreeMap, env, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, StatefulWidget, Wrap,
    },
};
use ratatui_image::{Resize, StatefulImage, picker::Picker, protocol::StatefulProtocol};
use serde_json::Value;
use sfumato_adapters::prompts::{LayeredPromptCatalog, PromptOverrideScope};
use sfumato_core::{
    application::SfumatoApplication,
    config::{
        Capability, ConfigOverrides, GlobalConfig, ModelDefaults, ModelOptions, ModelProfile,
    },
    config_editor::ConfigTarget,
    connectors::ConnectorPreset,
    prompts::{PromptCatalog, PromptId, PromptOrigin},
    providers::{GenerationStage, TextGenerationEvent},
    resources::slides::{EditSlidesResult, GenerateSlidesResult},
};
use sfumato_domain::SecretRef;
use tachyonfx::{EffectManager, fx};
use tokio::{
    sync::mpsc::{Receiver, Sender, channel},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tui_widgets::big_text::{BigText, PixelSize};

use crate::{
    cli::{EditSlidesArgs, SlidesArgs},
    commands::{execute_edit_slides, execute_slides},
};

const TICK_RATE: Duration = Duration::from_millis(80);
const NAV_ITEMS: &[(&str, &str)] = &[
    ("Generate", "Build a reviewed Marp deck"),
    ("Edit", "Update an existing generated deck"),
    ("Projects", "Project working directories"),
    ("Models", "Profiles, capabilities, defaults"),
    ("Connectors", "Local and cloud model endpoints"),
    ("Themes", "Reusable visual packages"),
    ("Prompts", "Layered model instructions"),
    ("Configuration", "Merged user and project settings"),
    ("Setup", "Initialize user and project settings"),
];

const BG: Color = Color::Rgb(24, 25, 26);
const PANEL: Color = Color::Rgb(35, 36, 38);
const TEXT: Color = Color::Rgb(224, 220, 211);
const MUTED: Color = Color::Rgb(146, 143, 137);
const ACCENT: Color = Color::Rgb(250, 189, 47);
const GREEN: Color = Color::Rgb(184, 187, 38);
const CYAN: Color = Color::Rgb(131, 165, 152);
const RED: Color = Color::Rgb(251, 73, 52);
const MAGENTA: Color = Color::Rgb(211, 134, 155);

pub async fn run(application: Arc<SfumatoApplication>) -> Result<()> {
    let mut terminal = ratatui::init();
    let _guard = TerminalGuard;
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let mut app = App::new(picker, application);
    let result = run_loop(&mut terminal, &mut app).await;
    app.shutdown().await;
    result
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

async fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(TICK_RATE);

    while !app.should_quit {
        if app.dirty {
            terminal.draw(|frame| app.draw(frame))?;
            app.dirty = false;
        }
        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        app.handle_key(key);
                        app.dirty = true;
                    }
                    Some(Err(error)) => return Err(error).context("Could not read terminal input"),
                    _ => {}
                }
            }
            _ = ticker.tick() => {
                if app.screen == Screen::Running || app.effects.is_running() {
                    app.tick();
                }
            },
            message = app.messages.recv() => {
                if let Some(message) = message {
                    app.handle_message(message);
                    app.dirty = true;
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Projects,
    Models,
    Connectors,
    Themes,
    Prompts,
    Configuration,
    Setup,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Models => "Models",
            Self::Connectors => "Connectors",
            Self::Themes => "Themes",
            Self::Prompts => "Prompts",
            Self::Configuration => "Configuration",
            Self::Setup => "Setup",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Home,
    Browse(Section),
    Generate,
    Edit,
    Running,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceOperation {
    Generate,
    Edit,
}

#[derive(Clone, Debug)]
struct BrowseRow {
    title: String,
    subtitle: String,
    detail: String,
    active: bool,
}

#[derive(Clone, Debug)]
enum FormField {
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
    Submit {
        label: &'static str,
    },
}

impl FormField {
    fn label(&self) -> &'static str {
        match self {
            Self::Text { label, .. } | Self::Toggle { label, .. } => label,
            Self::Submit { label } => label,
        }
    }
}

#[derive(Clone, Debug)]
struct GenerateForm {
    fields: Vec<FormField>,
    selected: usize,
}

#[derive(Clone, Debug)]
struct EditForm {
    fields: Vec<FormField>,
    selected: usize,
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
    fn text(&self, label: &str) -> String {
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

    fn to_args(&self) -> Result<EditSlidesArgs> {
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
        Self {
            fields: vec![
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
    fn text(&self, label: &str) -> String {
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

    fn toggle(&self, label: &str) -> bool {
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

    fn to_args(&self) -> Result<SlidesArgs> {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivityKind {
    Stage,
    Model,
    ToolCall,
    ToolResult,
    Warning,
    Success,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowseFocus {
    Actions,
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowseAction {
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
    ThemeUse,
    PromptCustomizeUser,
    PromptCustomizeProject,
    PromptValidate,
    ConfigSet,
    ConfigDelete,
    SetupUser,
}

impl BrowseAction {
    fn label(self) -> &'static str {
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
            Self::ThemeUse => "Apply",
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
enum OperationKind {
    ProjectCreate,
    ProjectRemove,
    ModelAdd,
    ModelEdit,
    ModelUse,
    ModelRemove,
    ConnectorSetup(ConnectorPreset),
    ThemeCreate,
    ThemeUse,
    PromptCustomize(PromptOverrideScope),
    PromptValidate,
    ConfigSet,
    ConfigDelete,
    SetupUser,
}

#[derive(Clone, Debug)]
struct OperationForm {
    title: &'static str,
    kind: OperationKind,
    target: Option<String>,
    fields: Vec<FormField>,
    selected: usize,
}

impl OperationForm {
    fn text(&self, label: &str) -> String {
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

    fn toggle(&self, label: &str) -> bool {
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
struct Activity {
    kind: ActivityKind,
    title: String,
    detail: String,
    image_path: Option<PathBuf>,
}

impl Activity {
    fn from_event(event: &TextGenerationEvent) -> Self {
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
}

enum UiMessage {
    GenerationEvent {
        job_id: u64,
        event: TextGenerationEvent,
    },
    ResourceFinished {
        job_id: u64,
        result: Box<Result<ResourceResult, String>>,
    },
    ResourceCancelled {
        job_id: u64,
    },
}

enum ResourceResult {
    Generated(GenerateSlidesResult),
    Edited(EditSlidesResult),
}

impl ResourceResult {
    fn markdown_path(&self) -> &std::path::Path {
        match self {
            Self::Generated(result) => &result.markdown_path,
            Self::Edited(result) => &result.markdown_path,
        }
    }

    fn warnings(&self) -> &[String] {
        match self {
            Self::Generated(result) => &result.warnings,
            Self::Edited(result) => &result.warnings,
        }
    }

    fn completion_message(&self) -> &'static str {
        match self {
            Self::Generated(_) => "Generation complete",
            Self::Edited(_) => "Slide edit complete",
        }
    }
}

struct App {
    application: Arc<SfumatoApplication>,
    screen: Screen,
    nav_index: usize,
    browse_rows: Vec<BrowseRow>,
    browse_index: usize,
    browse_focus: BrowseFocus,
    browse_action_index: usize,
    browse_detail_scroll: u16,
    operation: Option<OperationForm>,
    form: GenerateForm,
    edit_form: EditForm,
    resource_operation: ResourceOperation,
    activities: Vec<Activity>,
    activity_index: usize,
    current_stage: Option<GenerationStage>,
    generation_failed: bool,
    result: Option<ResourceResult>,
    status: Option<(String, bool)>,
    tick: usize,
    should_quit: bool,
    sender: Sender<UiMessage>,
    messages: Receiver<UiMessage>,
    next_job_id: u64,
    active_job_id: Option<u64>,
    cancellation: Option<CancellationToken>,
    active_task: Option<JoinHandle<()>>,
    picker: Picker,
    image: Option<StatefulProtocol>,
    effects: EffectManager<&'static str>,
    dirty: bool,
}

impl App {
    fn new(picker: Picker, application: Arc<SfumatoApplication>) -> Self {
        let (sender, messages) = channel(256);
        Self {
            application,
            screen: Screen::Home,
            nav_index: 0,
            browse_rows: Vec::new(),
            browse_index: 0,
            browse_focus: BrowseFocus::Rows,
            browse_action_index: 0,
            browse_detail_scroll: 0,
            operation: None,
            form: GenerateForm::default(),
            edit_form: EditForm::default(),
            resource_operation: ResourceOperation::Generate,
            activities: Vec::new(),
            activity_index: 0,
            current_stage: None,
            generation_failed: false,
            result: None,
            status: None,
            tick: 0,
            should_quit: false,
            sender,
            messages,
            next_job_id: 1,
            active_job_id: None,
            cancellation: None,
            active_task: None,
            picker,
            image: None,
            effects: EffectManager::default(),
            dirty: true,
        }
    }

    fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.dirty = true;
    }

    fn transition(&mut self, screen: Screen) {
        self.screen = screen;
        self.effects.add_unique_effect("screen", fx::coalesce(260));
        self.dirty = true;
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.cancel_active_job();
            self.should_quit = true;
            return;
        }
        if self.operation.is_some() {
            self.handle_operation_key(key);
            return;
        }
        match self.screen {
            Screen::Home => self.handle_home_key(key),
            Screen::Browse(section) => self.handle_browse_key(section, key),
            Screen::Generate => self.handle_generate_key(key),
            Screen::Edit => self.handle_edit_key(key),
            Screen::Running => self.handle_running_key(key),
            Screen::Complete => self.handle_complete_key(key),
        }
    }

    fn handle_home_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.nav_index = self.nav_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.nav_index = (self.nav_index + 1).min(NAV_ITEMS.len() - 1);
            }
            KeyCode::Enter => match self.nav_index {
                0 => self.transition(Screen::Generate),
                1 => self.transition(Screen::Edit),
                2 => self.open_section(Section::Projects),
                3 => self.open_section(Section::Models),
                4 => self.open_section(Section::Connectors),
                5 => self.open_section(Section::Themes),
                6 => self.open_section(Section::Prompts),
                7 => self.open_section(Section::Configuration),
                8 => self.open_section(Section::Setup),
                _ => {}
            },
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            _ => {}
        }
    }

    fn open_section(&mut self, section: Section) {
        match load_section(section, &self.application) {
            Ok(rows) => {
                self.browse_rows = rows;
                self.browse_index = 0;
                self.browse_focus = BrowseFocus::Actions;
                self.browse_action_index = 0;
                self.browse_detail_scroll = 0;
                self.status = None;
                self.transition(Screen::Browse(section));
            }
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    fn handle_browse_key(&mut self, section: Section, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Backspace => self.transition(Screen::Home),
            KeyCode::Tab | KeyCode::BackTab => {
                self.browse_focus = match self.browse_focus {
                    BrowseFocus::Actions => BrowseFocus::Rows,
                    BrowseFocus::Rows => BrowseFocus::Actions,
                };
            }
            KeyCode::Left | KeyCode::Char('h') if self.browse_focus == BrowseFocus::Actions => {
                self.browse_action_index = self.browse_action_index.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') if self.browse_focus == BrowseFocus::Actions => {
                self.browse_action_index = (self.browse_action_index + 1)
                    .min(section_actions(section).len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') if self.browse_focus == BrowseFocus::Rows => {
                self.browse_index = self.browse_index.saturating_sub(1);
                self.browse_detail_scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') if self.browse_focus == BrowseFocus::Rows => {
                self.browse_index =
                    (self.browse_index + 1).min(self.browse_rows.len().saturating_sub(1));
                self.browse_detail_scroll = 0;
            }
            KeyCode::PageUp => {
                self.browse_detail_scroll = self.browse_detail_scroll.saturating_sub(8);
            }
            KeyCode::PageDown => {
                self.browse_detail_scroll = self.browse_detail_scroll.saturating_add(8);
            }
            KeyCode::Enter if self.browse_focus == BrowseFocus::Actions => {
                self.execute_browse_action(section);
            }
            KeyCode::Char('r') => self.open_section(section),
            _ => {}
        }
    }

    fn execute_browse_action(&mut self, section: Section) {
        let Some(action) = section_actions(section)
            .get(self.browse_action_index)
            .copied()
        else {
            return;
        };
        if action == BrowseAction::ProjectActivate {
            self.activate_project();
            return;
        }
        match self.operation_for_action(action) {
            Ok(operation) => self.operation = Some(operation),
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    fn operation_for_action(&self, action: BrowseAction) -> Result<OperationForm> {
        let selected = self.browse_rows.get(self.browse_index);
        let operation = match action {
            BrowseAction::ProjectCreate => OperationForm {
                title: "Create project",
                kind: OperationKind::ProjectCreate,
                target: None,
                fields: vec![
                    text_field("Name", "", "university"),
                    text_field("Path", ".", "project working directory"),
                    FormField::Toggle {
                        label: "Make active",
                        value: true,
                    },
                    submit_field("Create project"),
                ],
                selected: 0,
            },
            BrowseAction::ProjectRemove => {
                let row = selected.context("Select a project to remove")?;
                confirmation_form(
                    "Remove project",
                    OperationKind::ProjectRemove,
                    row.title.clone(),
                    "Remove from registry",
                )
            }
            BrowseAction::ModelAdd => OperationForm {
                title: "Add model profile",
                kind: OperationKind::ModelAdd,
                target: None,
                fields: vec![
                    text_field("Name", "", "cloud-draft"),
                    text_field("Connector", "openrouter", "connector profile"),
                    text_field("Model ID", "", "provider model identifier"),
                    text_field("Capabilities", "text", "text, code, image"),
                    text_field("Options", "", "max_tokens=12000, temperature=0.4"),
                    submit_field("Add model"),
                ],
                selected: 0,
            },
            BrowseAction::ModelEdit => {
                let row = selected.context("Select a model profile to edit")?;
                let profile = self.application.show_model(&row.title)?;
                let capabilities = profile
                    .capabilities
                    .iter()
                    .map(|capability| capability.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let options = profile.options.cli_pairs().join(", ");
                OperationForm {
                    title: "Edit model profile",
                    kind: OperationKind::ModelEdit,
                    target: Some(row.title.clone()),
                    fields: vec![
                        text_field("Connector", &profile.connector, "connector profile"),
                        text_field("Model ID", &profile.model, "provider model identifier"),
                        text_field("Capabilities", &capabilities, "text, code, image"),
                        text_field("Options", &options, "key=value pairs"),
                        submit_field("Update model"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::ModelUse => {
                let profile = selected.map(|row| row.title.as_str()).unwrap_or("");
                OperationForm {
                    title: "Set model default",
                    kind: OperationKind::ModelUse,
                    target: None,
                    fields: vec![
                        text_field("Capability or role", "text", "text or reviewer"),
                        text_field("Profile", profile, "model profile"),
                        text_field("Project", "", "blank for user default"),
                        submit_field("Set default"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::ModelRemove => {
                let row = selected.context("Select a model profile to remove")?;
                confirmation_form(
                    "Remove model profile",
                    OperationKind::ModelRemove,
                    row.title.clone(),
                    "Remove model",
                )
            }
            BrowseAction::ConnectorOllama | BrowseAction::ConnectorOpenrouter => {
                let is_ollama = action == BrowseAction::ConnectorOllama;
                OperationForm {
                    title: if is_ollama {
                        "Setup Ollama"
                    } else {
                        "Setup OpenRouter"
                    },
                    kind: OperationKind::ConnectorSetup(if is_ollama {
                        ConnectorPreset::Ollama
                    } else {
                        ConnectorPreset::Openrouter
                    }),
                    target: None,
                    fields: vec![
                        text_field(
                            "Name",
                            if is_ollama { "ollama" } else { "openrouter" },
                            "connector name",
                        ),
                        text_field(
                            "API key environment",
                            "OPENROUTER_API_KEY",
                            "used by OpenRouter",
                        ),
                        submit_field("Save connector"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::ThemeCreate => OperationForm {
                title: "Create theme",
                kind: OperationKind::ThemeCreate,
                target: None,
                fields: vec![
                    text_field("Name", "", "gruvbox"),
                    submit_field("Create theme"),
                ],
                selected: 0,
            },
            BrowseAction::ThemeUse => {
                let theme = selected.context("Select a theme to apply")?;
                OperationForm {
                    title: "Apply theme",
                    kind: OperationKind::ThemeUse,
                    target: Some(theme.title.clone()),
                    fields: vec![
                        text_field("Project", "", "blank for active project"),
                        submit_field("Apply theme"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::PromptCustomizeUser | BrowseAction::PromptCustomizeProject => {
                let prompt = selected.context("Select a prompt to customize")?;
                let scope = if action == BrowseAction::PromptCustomizeUser {
                    PromptOverrideScope::User
                } else {
                    PromptOverrideScope::Project
                };
                confirmation_form(
                    "Customize prompt",
                    OperationKind::PromptCustomize(scope),
                    prompt.title.clone(),
                    "Create override",
                )
            }
            BrowseAction::PromptValidate => OperationForm {
                title: "Validate prompts",
                kind: OperationKind::PromptValidate,
                target: None,
                fields: vec![submit_field("Validate prompts")],
                selected: 0,
            },
            BrowseAction::ConfigSet => OperationForm {
                title: "Set configuration value",
                kind: OperationKind::ConfigSet,
                target: None,
                fields: vec![
                    text_field("Scope", "user", "user or project"),
                    text_field("Project", "", "blank for active project"),
                    text_field("Key", "", "dotted.key"),
                    text_field("Value", "", "TOML value or string"),
                    submit_field("Set value"),
                ],
                selected: 0,
            },
            BrowseAction::ConfigDelete => OperationForm {
                title: "Delete configuration value",
                kind: OperationKind::ConfigDelete,
                target: None,
                fields: vec![
                    text_field("Scope", "user", "user or project"),
                    text_field("Project", "", "blank for active project"),
                    text_field("Key", "", "dotted.key"),
                    FormField::Toggle {
                        label: "Confirm deletion",
                        value: false,
                    },
                    submit_field("Delete value"),
                ],
                selected: 0,
            },
            BrowseAction::SetupUser => OperationForm {
                title: "Initialize user",
                kind: OperationKind::SetupUser,
                target: None,
                fields: vec![
                    text_field(
                        "Name",
                        &env::var("USER").unwrap_or_else(|_| "Alex".to_string()),
                        "your name",
                    ),
                    text_field(
                        "Learning styles",
                        "visual, step-by-step",
                        "comma-separated preferences",
                    ),
                    text_field("Connector", "ollama", "ollama or openrouter"),
                    text_field("Profile", "local-text", "model profile name"),
                    text_field("Model ID", "llama3.2", "provider model identifier"),
                    text_field(
                        "API key environment",
                        "OPENROUTER_API_KEY",
                        "used by OpenRouter",
                    ),
                    FormField::Toggle {
                        label: "Overwrite existing config",
                        value: false,
                    },
                    submit_field("Initialize user"),
                ],
                selected: 0,
            },
            BrowseAction::ProjectActivate => unreachable!("project activation is immediate"),
        };
        Ok(operation)
    }

    fn activate_project(&mut self) {
        let Some(row) = self.browse_rows.get(self.browse_index) else {
            return;
        };
        match self.application.use_project(&row.title) {
            Ok(name) => {
                self.open_section(Section::Projects);
                self.status = Some((format!("Active project: {name}"), false));
            }
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    fn handle_operation_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.operation = None;
            return;
        }
        let Some(operation) = &mut self.operation else {
            return;
        };
        match key.code {
            KeyCode::Up => operation.selected = operation.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                operation.selected =
                    (operation.selected + 1).min(operation.fields.len().saturating_sub(1));
            }
            KeyCode::BackTab => operation.selected = operation.selected.saturating_sub(1),
            KeyCode::Enter => match operation.fields.get(operation.selected) {
                Some(FormField::Toggle { .. }) => {
                    if let Some(FormField::Toggle { value, .. }) =
                        operation.fields.get_mut(operation.selected)
                    {
                        *value = !*value;
                    }
                }
                Some(FormField::Submit { .. }) => self.submit_operation(),
                _ => {
                    operation.selected =
                        (operation.selected + 1).min(operation.fields.len().saturating_sub(1));
                }
            },
            KeyCode::Char(' ')
                if matches!(
                    operation.fields.get(operation.selected),
                    Some(FormField::Toggle { .. })
                ) =>
            {
                if let Some(FormField::Toggle { value, .. }) =
                    operation.fields.get_mut(operation.selected)
                {
                    *value = !*value;
                }
            }
            KeyCode::Backspace => {
                if let Some(FormField::Text { value, .. }) =
                    operation.fields.get_mut(operation.selected)
                {
                    value.pop();
                }
            }
            KeyCode::Char(character) => {
                if let Some(FormField::Text { value, .. }) =
                    operation.fields.get_mut(operation.selected)
                {
                    value.push(character);
                }
            }
            _ => {}
        }
    }

    fn submit_operation(&mut self) {
        let Some(operation) = self.operation.clone() else {
            return;
        };
        let section = match self.screen {
            Screen::Browse(section) => section,
            _ => return,
        };
        match execute_operation(&operation, &self.application) {
            Ok(message) => {
                self.operation = None;
                self.open_section(section);
                self.status = Some((message, false));
            }
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    fn handle_generate_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.transition(Screen::Home),
            KeyCode::Up => self.form.selected = self.form.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                self.form.selected = (self.form.selected + 1).min(self.form.fields.len() - 1)
            }
            KeyCode::BackTab => self.form.selected = self.form.selected.saturating_sub(1),
            KeyCode::Enter => {
                let selected = self.form.selected;
                match self.form.fields.get(selected) {
                    Some(FormField::Toggle { .. }) => self.toggle_form_field(),
                    Some(FormField::Submit { .. }) => self.start_generation(),
                    Some(FormField::Text {
                        multiline: true, ..
                    }) if key.modifiers.contains(KeyModifiers::SHIFT) => self.push_form_char('\n'),
                    _ => {
                        self.form.selected =
                            (self.form.selected + 1).min(self.form.fields.len() - 1)
                    }
                }
            }
            KeyCode::Char(' ')
                if matches!(
                    self.form.fields.get(self.form.selected),
                    Some(FormField::Toggle { .. })
                ) =>
            {
                self.toggle_form_field();
            }
            KeyCode::Backspace => {
                if let Some(FormField::Text { value, .. }) =
                    self.form.fields.get_mut(self.form.selected)
                {
                    value.pop();
                }
            }
            KeyCode::Char(character) => self.push_form_char(character),
            _ => {}
        }
    }

    fn push_form_char(&mut self, character: char) {
        if let Some(FormField::Text { value, .. }) = self.form.fields.get_mut(self.form.selected) {
            value.push(character);
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.transition(Screen::Home),
            KeyCode::Up => {
                self.edit_form.selected = self.edit_form.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Tab => {
                self.edit_form.selected =
                    (self.edit_form.selected + 1).min(self.edit_form.fields.len() - 1);
            }
            KeyCode::BackTab => {
                self.edit_form.selected = self.edit_form.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                let selected = self.edit_form.selected;
                match self.edit_form.fields.get(selected) {
                    Some(FormField::Submit { .. }) => self.start_edit(),
                    Some(FormField::Text {
                        multiline: true, ..
                    }) if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        self.push_edit_form_char('\n');
                    }
                    _ => {
                        self.edit_form.selected =
                            (self.edit_form.selected + 1).min(self.edit_form.fields.len() - 1);
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(FormField::Text { value, .. }) =
                    self.edit_form.fields.get_mut(self.edit_form.selected)
                {
                    value.pop();
                }
            }
            KeyCode::Char(character) => self.push_edit_form_char(character),
            _ => {}
        }
    }

    fn push_edit_form_char(&mut self, character: char) {
        if let Some(FormField::Text { value, .. }) =
            self.edit_form.fields.get_mut(self.edit_form.selected)
        {
            value.push(character);
        }
    }

    fn toggle_form_field(&mut self) {
        if let Some(FormField::Toggle { value, .. }) = self.form.fields.get_mut(self.form.selected)
        {
            *value = !*value;
        }
    }

    fn start_generation(&mut self) {
        let args = match self.form.to_args() {
            Ok(args) => args,
            Err(error) => {
                self.status = Some((error.to_string(), true));
                return;
            }
        };
        self.activities.clear();
        self.activity_index = 0;
        self.current_stage = None;
        self.generation_failed = false;
        self.result = None;
        self.image = None;
        self.status = None;
        self.resource_operation = ResourceOperation::Generate;
        self.transition(Screen::Running);

        let job_id = self.begin_job();
        let cancellation = self
            .cancellation
            .as_ref()
            .expect("a started job has a cancellation token")
            .clone();
        let sender = self.sender.clone();
        let application = Arc::clone(&self.application);
        let sink_sender = sender.clone();
        let sink = Arc::new(move |event| {
            let _ = sink_sender.try_send(UiMessage::GenerationEvent { job_id, event });
        });
        self.active_task = Some(tokio::spawn(async move {
            tokio::select! {
                () = cancellation.cancelled() => {
                    let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
                }
                result = execute_slides(&application, args, Some(sink)) => {
                    let result = result
                        .map(ResourceResult::Generated)
                        .map_err(|error| format!("{error:#}"));
                    let _ = sender.send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    }).await;
                }
            }
        }));
    }

    fn start_edit(&mut self) {
        let args = match self.edit_form.to_args() {
            Ok(args) => args,
            Err(error) => {
                self.status = Some((error.to_string(), true));
                return;
            }
        };
        self.activities.clear();
        self.activity_index = 0;
        self.current_stage = None;
        self.generation_failed = false;
        self.result = None;
        self.image = None;
        self.status = None;
        self.resource_operation = ResourceOperation::Edit;
        self.transition(Screen::Running);

        let job_id = self.begin_job();
        let cancellation = self
            .cancellation
            .as_ref()
            .expect("a started job has a cancellation token")
            .clone();
        let sender = self.sender.clone();
        let application = Arc::clone(&self.application);
        let sink_sender = sender.clone();
        let sink = Arc::new(move |event| {
            let _ = sink_sender.try_send(UiMessage::GenerationEvent { job_id, event });
        });
        self.active_task = Some(tokio::spawn(async move {
            tokio::select! {
                () = cancellation.cancelled() => {
                    let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
                }
                result = execute_edit_slides(&application, args, Some(sink)) => {
                    let result = result
                        .map(ResourceResult::Edited)
                        .map_err(|error| format!("{error:#}"));
                    let _ = sender.send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    }).await;
                }
            }
        }));
    }

    fn begin_job(&mut self) -> u64 {
        self.cancel_active_job();
        let job_id = self.next_job_id;
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        self.active_job_id = Some(job_id);
        self.cancellation = Some(CancellationToken::new());
        job_id
    }

    fn cancel_active_job(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.cancel();
        }
    }

    async fn shutdown(&mut self) {
        self.cancel_active_job();
        if let Some(task) = self.active_task.take() {
            let _ = task.await;
        }
    }

    fn handle_running_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.cancel_active_job();
                self.status = Some(("Cancelling the active operation...".to_string(), false));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.activity_index = self.activity_index.saturating_sub(1);
                self.load_selected_image();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.activity_index =
                    (self.activity_index + 1).min(self.activities.len().saturating_sub(1));
                self.load_selected_image();
            }
            _ => {}
        }
    }

    fn handle_complete_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Backspace => self.transition(Screen::Home),
            KeyCode::Enter => self.transition(match self.resource_operation {
                ResourceOperation::Generate => Screen::Generate,
                ResourceOperation::Edit => Screen::Edit,
            }),
            KeyCode::Up | KeyCode::Char('k') => {
                self.activity_index = self.activity_index.saturating_sub(1);
                self.load_selected_image();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.activity_index =
                    (self.activity_index + 1).min(self.activities.len().saturating_sub(1));
                self.load_selected_image();
            }
            _ => {}
        }
    }

    fn handle_message(&mut self, message: UiMessage) {
        match message {
            UiMessage::GenerationEvent { job_id, event } => {
                if self.active_job_id != Some(job_id) {
                    return;
                }
                if let TextGenerationEvent::StageStarted { stage, .. } = &event {
                    self.current_stage = Some(*stage);
                }
                let activity = Activity::from_event(&event);
                let image_path = activity.image_path.clone();
                self.activities.push(activity);
                self.activity_index = self.activities.len().saturating_sub(1);
                if let Some(path) = image_path {
                    self.load_image(&path);
                }
            }
            UiMessage::ResourceFinished { job_id, result } => {
                if self.active_job_id != Some(job_id) {
                    return;
                }
                self.active_job_id = None;
                self.cancellation = None;
                self.active_task = None;
                match *result {
                    Ok(result) => {
                        for warning in result.warnings() {
                            self.activities.push(Activity {
                                kind: ActivityKind::Warning,
                                title: "Resource warning".to_string(),
                                detail: warning.clone(),
                                image_path: None,
                            });
                        }
                        self.status = Some((result.completion_message().to_string(), false));
                        self.result = Some(result);
                    }
                    Err(error) => {
                        self.generation_failed = true;
                        self.activities.push(Activity {
                            kind: ActivityKind::Warning,
                            title: "Resource operation failed".to_string(),
                            detail: error.clone(),
                            image_path: None,
                        });
                        self.status = Some((error, true));
                    }
                }
                self.activity_index = self.activities.len().saturating_sub(1);
                self.transition(Screen::Complete);
            }
            UiMessage::ResourceCancelled { job_id } => {
                if self.active_job_id != Some(job_id) {
                    return;
                }
                self.active_job_id = None;
                self.cancellation = None;
                self.active_task = None;
                self.status = Some(("Operation cancelled".to_string(), false));
                self.activities.push(Activity {
                    kind: ActivityKind::Warning,
                    title: "Operation cancelled".to_string(),
                    detail: "No staged artifacts were committed.".to_string(),
                    image_path: None,
                });
                self.activity_index = self.activities.len().saturating_sub(1);
                self.transition(Screen::Complete);
            }
        }
    }

    fn load_selected_image(&mut self) {
        let path = self
            .activities
            .get(self.activity_index)
            .and_then(|activity| activity.image_path.clone());
        if let Some(path) = path {
            self.load_image(&path);
        } else {
            self.image = None;
        }
    }

    fn load_image(&mut self, path: &std::path::Path) {
        match image::open(path) {
            Ok(image) => self.image = Some(self.picker.new_resize_protocol(image)),
            Err(error) => {
                self.image = None;
                self.status = Some((
                    format!("Could not preview {}: {error}", path.display()),
                    true,
                ));
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.render_widget(Block::new().style(Style::default().bg(BG).fg(TEXT)), area);
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(if area.height >= 24 { 6 } else { 3 }),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .areas(area);
        self.draw_header(frame, header);
        match self.screen {
            Screen::Home => self.draw_home(frame, body),
            Screen::Browse(section) => self.draw_browse(frame, body, section),
            Screen::Generate => self.draw_generate(frame, body),
            Screen::Edit => self.draw_edit(frame, body),
            Screen::Running | Screen::Complete => self.draw_generation(frame, body),
        }
        if self.operation.is_some() {
            self.draw_operation(frame, body);
        }
        self.draw_footer(frame, footer);
        self.effects
            .process_effects(TICK_RATE.into(), frame.buffer_mut(), area);
    }

    fn draw_header(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.height >= 6 && area.width >= 100 {
            let [brand, context] =
                Layout::horizontal([Constraint::Length(60), Constraint::Min(10)])
                    .areas(area.inner(Margin::new(2, 0)));
            let logo = BigText::builder()
                .pixel_size(PixelSize::HalfHeight)
                .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
                .lines(vec![Line::from("SFUMATO")])
                .build();
            frame.render_widget(logo, brand);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "STUDY RESOURCE ENGINE",
                        Style::default().fg(CYAN),
                    )),
                    Line::from(Span::styled(self.breadcrumb(), Style::default().fg(MUTED))),
                ])
                .alignment(Alignment::Right),
                context,
            );
        } else {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" SFUMATO ", Style::default().fg(BG).bg(ACCENT).bold()),
                    Span::raw("  "),
                    Span::styled(self.breadcrumb(), Style::default().fg(MUTED)),
                ])),
                area,
            );
        }
    }

    fn breadcrumb(&self) -> String {
        match self.screen {
            Screen::Home => "Workspace".to_string(),
            Screen::Browse(section) => section.title().to_string(),
            Screen::Generate => "Generate / Slides".to_string(),
            Screen::Edit => "Edit / Slides".to_string(),
            Screen::Running => match self.resource_operation {
                ResourceOperation::Generate => "Generate / In progress".to_string(),
                ResourceOperation::Edit => "Edit / In progress".to_string(),
            },
            Screen::Complete => match self.resource_operation {
                ResourceOperation::Generate => "Generate / Result".to_string(),
                ResourceOperation::Edit => "Edit / Result".to_string(),
            },
        }
    }

    fn draw_home(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let [menu_area, context_area] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(area.inner(Margin::new(2, 0)));
        let items = NAV_ITEMS
            .iter()
            .enumerate()
            .map(|(index, (title, subtitle))| {
                let marker = if index == self.nav_index { ">" } else { " " };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{marker} "), Style::default().fg(ACCENT)),
                        Span::styled(*title, Style::default().fg(TEXT).bold()),
                    ]),
                    Line::from(Span::styled(
                        format!("   {subtitle}"),
                        Style::default().fg(MUTED),
                    )),
                ])
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default().with_selected(Some(self.nav_index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(PANEL))
                .block(panel("WORKSPACE")),
            menu_area,
            &mut state,
        );

        let project = self
            .application
            .list_projects()
            .ok()
            .and_then(|projects| projects.into_iter().find(|project| project.active));
        let project_name = project
            .as_ref()
            .map(|project| project.name.as_str())
            .unwrap_or("No active project");
        let project_path = project
            .as_ref()
            .map(|project| project.path.display().to_string())
            .unwrap_or_else(|| "Create or activate a project".to_string());
        let models = self
            .application
            .list_models()
            .map(|models| models.len())
            .unwrap_or(0);
        let connectors = self
            .application
            .list_connectors()
            .map(|connectors| connectors.len())
            .unwrap_or(0);
        let themes = self
            .application
            .list_themes()
            .map(|themes| themes.len())
            .unwrap_or(0);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "ACTIVE PROJECT",
                    Style::default().fg(CYAN).bold(),
                )),
                Line::from(Span::styled(project_name, Style::default().fg(TEXT).bold())),
                Line::from(Span::styled(project_path, Style::default().fg(MUTED))),
                Line::from(""),
                metric_line("Model profiles", models),
                metric_line("Connectors", connectors),
                metric_line("Themes", themes),
            ])
            .wrap(Wrap { trim: true })
            .block(panel("CONTEXT")),
            context_area,
        );
    }

    fn draw_browse(&mut self, frame: &mut Frame<'_>, area: Rect, section: Section) {
        let [actions_area, content_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(5)])
                .areas(area.inner(Margin::new(2, 0)));
        let action_spans = section_actions(section)
            .iter()
            .enumerate()
            .flat_map(|(index, action)| {
                let selected =
                    self.browse_focus == BrowseFocus::Actions && index == self.browse_action_index;
                [
                    Span::styled(
                        format!(" {} ", action.label()),
                        if selected {
                            Style::default().fg(BG).bg(ACCENT).bold()
                        } else {
                            Style::default().fg(TEXT).bg(PANEL)
                        },
                    ),
                    Span::raw(" "),
                ]
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Line::from(action_spans)).block(panel("ACTIONS")),
            actions_area,
        );
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(content_area);
        let items = if self.browse_rows.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "No entries",
                Style::default().fg(MUTED),
            )))]
        } else {
            self.browse_rows
                .iter()
                .map(|row| {
                    let marker = if row.active { "*" } else { " " };
                    ListItem::new(vec![
                        Line::from(vec![
                            Span::styled(format!("{marker} "), Style::default().fg(GREEN)),
                            Span::styled(&row.title, Style::default().fg(TEXT).bold()),
                        ]),
                        Line::from(Span::styled(
                            format!("  {}", row.subtitle),
                            Style::default().fg(MUTED),
                        )),
                    ])
                })
                .collect()
        };
        let mut state = ListState::default()
            .with_selected((!self.browse_rows.is_empty()).then_some(self.browse_index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(PANEL))
                .block(panel(section.title())),
            list_area,
            &mut state,
        );
        let detail = self
            .browse_rows
            .get(self.browse_index)
            .map(|row| row.detail.as_str())
            .unwrap_or("Nothing selected");
        frame.render_widget(
            Paragraph::new(detail)
                .style(Style::default().fg(TEXT))
                .wrap(Wrap { trim: false })
                .scroll((self.browse_detail_scroll, 0))
                .block(panel("DETAIL")),
            detail_area,
        );
    }

    fn draw_operation(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(operation) = &self.operation else {
            return;
        };
        let width = area.width.saturating_sub(8).min(72);
        let height = (operation.fields.len() as u16 * 3 + 2)
            .min(area.height.saturating_sub(2))
            .max(5);
        let modal = centered_rect(width, height, area);
        frame.render_widget(Clear, modal);
        frame.render_widget(
            Block::new()
                .title(format!(" {} ", operation.title))
                .title_style(Style::default().fg(ACCENT).bold())
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(ACCENT))
                .style(Style::default().bg(BG)),
            modal,
        );
        let content = modal.inner(Margin::new(2, 1));
        let range = visible_field_range(&operation.fields, operation.selected, content.height);
        let visible_fields = &operation.fields[range.clone()];
        let rows = Layout::vertical(
            visible_fields
                .iter()
                .map(|field| Constraint::Length(field_height(field)))
                .collect::<Vec<_>>(),
        )
        .split(content);
        for (offset, (field, row)) in visible_fields.iter().zip(rows.iter()).enumerate() {
            let index = range.start + offset;
            let selected = index == operation.selected;
            match field {
                FormField::Text {
                    label,
                    value,
                    placeholder,
                    ..
                } => {
                    let text = if value.is_empty() {
                        Span::styled(*placeholder, Style::default().fg(MUTED))
                    } else {
                        Span::styled(value.as_str(), Style::default().fg(TEXT))
                    };
                    frame.render_widget(
                        Paragraph::new(text).block(field_block(label, selected)),
                        *row,
                    );
                }
                FormField::Toggle { label, value } => {
                    let symbol = if *value { "[x]" } else { "[ ]" };
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled(
                                symbol,
                                Style::default().fg(if *value { GREEN } else { MUTED }),
                            ),
                            Span::raw(" "),
                            Span::styled(*label, Style::default().fg(TEXT)),
                        ]))
                        .block(field_block("OPTION", selected)),
                        *row,
                    );
                }
                FormField::Submit { label } => {
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            *label,
                            Style::default().fg(if selected { BG } else { TEXT }).bold(),
                        ))
                        .alignment(Alignment::Center)
                        .style(if selected {
                            Style::default().bg(ACCENT)
                        } else {
                            Style::default().bg(PANEL)
                        })
                        .block(Block::new().borders(Borders::ALL)),
                        *row,
                    );
                }
            }
        }
    }

    fn draw_generate(&mut self, frame: &mut Frame<'_>, area: Rect) {
        draw_resource_form(frame, area, &self.form.fields, self.form.selected);
    }

    fn draw_edit(&mut self, frame: &mut Frame<'_>, area: Rect) {
        draw_resource_form(frame, area, &self.edit_form.fields, self.edit_form.selected);
    }

    fn draw_generation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let inner = area.inner(Margin::new(2, 0));
        let [stages_area, main_area] =
            Layout::horizontal([Constraint::Length(24), Constraint::Min(30)]).areas(inner);
        self.draw_stages(frame, stages_area);
        let has_image = self.image.is_some() && main_area.width >= 70;
        let [activity_area, preview_area] = if has_image {
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .areas(main_area)
        } else {
            [main_area, Rect::default()]
        };
        self.draw_activity(frame, activity_area);
        if has_image {
            let preview = Block::new()
                .title(" IMAGE PREVIEW ")
                .title_style(Style::default().fg(MAGENTA).bold())
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(MUTED));
            let content = preview.inner(preview_area).inner(Margin::new(1, 1));
            frame.render_widget(preview, preview_area);
            if let Some(image) = &mut self.image {
                StatefulImage::default().resize(Resize::Fit(None)).render(
                    content,
                    frame.buffer_mut(),
                    image,
                );
            }
        }
    }

    fn draw_stages(&self, frame: &mut Frame<'_>, area: Rect) {
        let generation_stages = [
            GenerationStage::Draft,
            GenerationStage::SemanticReview,
            GenerationStage::LayoutCheck,
            GenerationStage::LayoutRepair,
            GenerationStage::Rendering,
        ];
        let edit_stages = [
            GenerationStage::Edit,
            GenerationStage::LayoutCheck,
            GenerationStage::Rendering,
        ];
        let stages: &[GenerationStage] = match self.resource_operation {
            ResourceOperation::Generate => &generation_stages,
            ResourceOperation::Edit => &edit_stages,
        };
        let current = self
            .current_stage
            .and_then(|current| stages.iter().position(|stage| *stage == current))
            .unwrap_or(0);
        let running = self.screen == Screen::Running;
        let completed = self.screen == Screen::Complete && !self.generation_failed;
        let spinner = ["|", "/", "-", "\\"][self.tick % 4];
        let lines = stages
            .iter()
            .enumerate()
            .map(|(index, stage)| {
                let (marker, color) = if index < current || completed {
                    ("+", GREEN)
                } else if index == current && running {
                    (spinner, ACCENT)
                } else if index == current && self.generation_failed {
                    ("!", RED)
                } else {
                    (".", MUTED)
                };
                Line::from(vec![
                    Span::styled(format!(" {marker} "), Style::default().fg(color).bold()),
                    Span::styled(
                        stage_label(*stage),
                        Style::default().fg(if index <= current { TEXT } else { MUTED }),
                    ),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).block(panel("PIPELINE")), area);
    }

    fn draw_activity(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let items = self
            .activities
            .iter()
            .map(|activity| {
                let (marker, color) = activity_style(activity.kind);
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{marker} "), Style::default().fg(color).bold()),
                        Span::styled(&activity.title, Style::default().fg(TEXT).bold()),
                    ]),
                    Line::from(Span::styled(
                        format!("  {}", compact(&activity.detail, 180)),
                        Style::default().fg(MUTED),
                    )),
                ])
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default()
            .with_selected((!self.activities.is_empty()).then_some(self.activity_index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(PANEL))
                .block(panel(if self.screen == Screen::Running {
                    "ACTIVITY"
                } else {
                    "RESULT"
                })),
            area,
            &mut state,
        );
        if self.activities.len() > area.height.saturating_sub(2) as usize {
            let mut scrollbar =
                ScrollbarState::new(self.activities.len()).position(self.activity_index);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight),
                area.inner(Margin::new(0, 1)),
                &mut scrollbar,
            );
        }
    }

    fn draw_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let (message, error) = self.status.as_ref().map_or_else(
            || {
                let message = match self.screen {
                    Screen::Running => "Resource operation is running".to_string(),
                    Screen::Complete if self.result.is_some() => self
                        .result
                        .as_ref()
                        .map(|result| format!("Artifact: {}", result.markdown_path().display()))
                        .unwrap_or_default(),
                    _ => "Ready".to_string(),
                };
                (message, false)
            },
            |(message, error)| (message.clone(), *error),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", Style::default().bg(if error { RED } else { CYAN })),
                Span::raw(" "),
                Span::styled(
                    compact(&message, area.width.saturating_sub(4) as usize),
                    Style::default().fg(if error { RED } else { MUTED }),
                ),
            ])),
            area,
        );
    }
}

fn draw_resource_form(
    frame: &mut Frame<'_>,
    area: Rect,
    fields: &[FormField],
    selected_index: usize,
) {
    let form_area = area.inner(Margin::new(3, 0));
    let range = visible_field_range(fields, selected_index, form_area.height);
    let visible_fields = &fields[range.clone()];
    let rows = Layout::vertical(
        visible_fields
            .iter()
            .map(|field| Constraint::Length(field_height(field)))
            .collect::<Vec<_>>(),
    )
    .split(form_area);
    for (offset, (field, row)) in visible_fields.iter().zip(rows.iter()).enumerate() {
        let index = range.start + offset;
        let selected = index == selected_index;
        match field {
            FormField::Text {
                label,
                value,
                placeholder,
                ..
            } => {
                let text = if value.is_empty() {
                    Span::styled(*placeholder, Style::default().fg(MUTED))
                } else {
                    Span::styled(value.as_str(), Style::default().fg(TEXT))
                };
                frame.render_widget(
                    Paragraph::new(text)
                        .wrap(Wrap { trim: false })
                        .block(field_block(label, selected)),
                    *row,
                );
            }
            FormField::Toggle { label, value } => {
                let symbol = if *value { "[x]" } else { "[ ]" };
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            symbol,
                            Style::default().fg(if *value { GREEN } else { MUTED }),
                        ),
                        Span::raw(" "),
                        Span::styled(*label, Style::default().fg(TEXT)),
                    ]))
                    .block(field_block("OPTION", selected)),
                    *row,
                );
            }
            FormField::Submit { .. } => {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        field.label(),
                        Style::default().fg(if selected { BG } else { TEXT }).bold(),
                    )))
                    .alignment(Alignment::Center)
                    .style(if selected {
                        Style::default().bg(ACCENT)
                    } else {
                        Style::default().bg(PANEL)
                    })
                    .block(
                        Block::new()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded),
                    ),
                    *row,
                );
            }
        }
    }
}

fn section_actions(section: Section) -> &'static [BrowseAction] {
    match section {
        Section::Projects => &[
            BrowseAction::ProjectCreate,
            BrowseAction::ProjectActivate,
            BrowseAction::ProjectRemove,
        ],
        Section::Models => &[
            BrowseAction::ModelAdd,
            BrowseAction::ModelEdit,
            BrowseAction::ModelUse,
            BrowseAction::ModelRemove,
        ],
        Section::Connectors => &[
            BrowseAction::ConnectorOllama,
            BrowseAction::ConnectorOpenrouter,
        ],
        Section::Themes => &[BrowseAction::ThemeCreate, BrowseAction::ThemeUse],
        Section::Prompts => &[
            BrowseAction::PromptCustomizeUser,
            BrowseAction::PromptCustomizeProject,
            BrowseAction::PromptValidate,
        ],
        Section::Configuration => &[BrowseAction::ConfigSet, BrowseAction::ConfigDelete],
        Section::Setup => &[BrowseAction::SetupUser, BrowseAction::ProjectCreate],
    }
}

fn text_field(label: &'static str, value: &str, placeholder: &'static str) -> FormField {
    FormField::Text {
        label,
        value: value.to_string(),
        placeholder,
        multiline: false,
    }
}

fn submit_field(label: &'static str) -> FormField {
    FormField::Submit { label }
}

fn confirmation_form(
    title: &'static str,
    kind: OperationKind,
    target: String,
    submit_label: &'static str,
) -> OperationForm {
    OperationForm {
        title,
        kind,
        target: Some(target),
        fields: vec![
            FormField::Toggle {
                label: "Confirm",
                value: false,
            },
            submit_field(submit_label),
        ],
        selected: 0,
    }
}

fn required_field(form: &OperationForm, label: &str) -> Result<String> {
    let value = form.text(label);
    if value.is_empty() {
        anyhow::bail!("{label} cannot be empty");
    }
    Ok(value)
}

fn optional_field(form: &OperationForm, label: &str) -> Option<String> {
    let value = form.text(label);
    (!value.is_empty()).then_some(value)
}

fn config_target(value: &str) -> Result<ConfigTarget> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Ok(ConfigTarget::User),
        "project" => Ok(ConfigTarget::Project),
        _ => anyhow::bail!("Scope must be 'user' or 'project'"),
    }
}

fn execute_operation(form: &OperationForm, application: &SfumatoApplication) -> Result<String> {
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
            let path = tui_prompt_catalog(application)?.customize(id, scope)?;
            Ok(format!("Created prompt override at {}", path.display()))
        }
        OperationKind::PromptValidate => {
            let prompts = tui_prompt_catalog(application)?.validate()?;
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
                        temperature: Some(0.4),
                        max_tokens: Some(4000),
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

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

fn field_height(field: &FormField) -> u16 {
    match field {
        FormField::Text {
            multiline: true, ..
        } => 4,
        _ => 3,
    }
}

fn visible_field_range(
    fields: &[FormField],
    selected: usize,
    available_height: u16,
) -> std::ops::Range<usize> {
    if fields.is_empty() || available_height == 0 {
        return 0..0;
    }
    let selected = selected.min(fields.len() - 1);
    let mut start = selected;
    let mut used = field_height(&fields[selected]);
    while start > 0 {
        let previous = field_height(&fields[start - 1]);
        if used.saturating_add(previous) > available_height {
            break;
        }
        start -= 1;
        used += previous;
    }
    let mut end = selected + 1;
    while end < fields.len() {
        let next = field_height(&fields[end]);
        if used.saturating_add(next) > available_height {
            break;
        }
        used += next;
        end += 1;
    }
    start..end
}

fn load_section(section: Section, application: &SfumatoApplication) -> Result<Vec<BrowseRow>> {
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
        Section::Prompts => {
            let catalog = tui_prompt_catalog(application)?;
            catalog
                .list()?
                .into_iter()
                .map(|template| {
                    let (source, provenance) = catalog.source(template.id)?;
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
                            provenance.content_hash, provenance.version, source
                        ),
                        active,
                    })
                })
                .collect()
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

fn tui_prompt_catalog(application: &SfumatoApplication) -> Result<LayeredPromptCatalog> {
    let config = application.resolve_config(ConfigOverrides::default())?;
    Ok(LayeredPromptCatalog::for_project(config.project_root)?)
}

fn panel(title: &'static str) -> Block<'static> {
    Block::new()
        .title(format!(" {title} "))
        .title_style(Style::default().fg(CYAN).bold())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .style(Style::default().bg(BG))
}

fn field_block(label: &'static str, selected: bool) -> Block<'static> {
    Block::new()
        .title(format!(" {label} "))
        .title_style(
            Style::default()
                .fg(if selected { ACCENT } else { MUTED })
                .bold(),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if selected { ACCENT } else { MUTED }))
}

fn metric_line(label: &str, value: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{value:>3}"), Style::default().fg(ACCENT).bold()),
        Span::styled(format!("  {label}"), Style::default().fg(MUTED)),
    ])
}

fn stage_label(stage: GenerationStage) -> &'static str {
    match stage {
        GenerationStage::Draft => "Draft",
        GenerationStage::Edit => "Content edit",
        GenerationStage::SemanticReview => "Content review",
        GenerationStage::LayoutCheck => "Layout check",
        GenerationStage::LayoutRepair => "Layout repair",
        GenerationStage::Rendering => "Rendering",
    }
}

fn activity_style(kind: ActivityKind) -> (&'static str, Color) {
    match kind {
        ActivityKind::Stage => (">", ACCENT),
        ActivityKind::Model => ("~", CYAN),
        ActivityKind::ToolCall => (">", MAGENTA),
        ActivityKind::ToolResult => ("+", GREEN),
        ActivityKind::Warning => ("!", RED),
        ActivityKind::Success => ("+", GREEN),
    }
}

fn tool_name(name: &str) -> String {
    match name {
        "sfumato_list_directory" => "List directory".to_string(),
        "sfumato_read_file" => "Read file".to_string(),
        "sfumato_image_gen" => "Generate image".to_string(),
        _ => name.replace('_', " "),
    }
}

fn format_tool_arguments(arguments: &Value) -> String {
    let arguments = parse_maybe_json_string(arguments);
    if let Some(path) = arguments.get("path").and_then(Value::as_str) {
        return path.to_string();
    }
    if let Some(prompt) = arguments.get("prompt").and_then(Value::as_str) {
        return compact(prompt, 220);
    }
    compact(&arguments.to_string(), 220)
}

fn format_tool_result(name: &str, result: &str) -> (String, Option<PathBuf>) {
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return (compact(result, 260), None);
    };
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return (format!("Error: {}", compact(error, 240)), None);
    }
    if name == "sfumato_list_directory" || value.get("entries").is_some() {
        return (summarize_directory(&value), None);
    }
    if name == "sfumato_read_file" || value.get("content").is_some() {
        return (summarize_file(&value), None);
    }
    if name == "sfumato_image_gen" || value.get("markdown_path").is_some() {
        let markdown_path = value
            .get("markdown_path")
            .and_then(Value::as_str)
            .unwrap_or("generated image");
        let profile = value
            .get("model_profile")
            .and_then(Value::as_str)
            .map(|profile| format!(" with {profile}"))
            .unwrap_or_default();
        let path = value.get("path").and_then(Value::as_str).map(PathBuf::from);
        return (format!("Created {markdown_path}{profile}"), path);
    }
    (compact(result, 260), None)
}

fn summarize_directory(value: &Value) -> String {
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("directory");
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let files = entries
        .iter()
        .filter(|entry| entry.get("kind").and_then(Value::as_str) == Some("file"))
        .count();
    let directories = entries.len().saturating_sub(files);
    let names = entries
        .iter()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .take(6)
        .collect::<Vec<_>>()
        .join(", ");
    let names = if names.is_empty() {
        String::new()
    } else {
        format!(" - {names}")
    };
    format!(
        "Listed {path}: {} entries ({files} files, {directories} directories){}",
        entries.len(),
        names
    )
}

fn summarize_file(value: &Value) -> String {
    let path = value.get("path").and_then(Value::as_str).unwrap_or("file");
    let content = value.get("content").and_then(Value::as_str).unwrap_or("");
    format!(
        "Read {path}: {} characters, {} lines",
        content.chars().count(),
        content.lines().count()
    )
}

fn parse_maybe_json_string(value: &Value) -> Value {
    if let Value::String(raw) = value {
        serde_json::from_str(raw).unwrap_or_else(|_| value.clone())
    } else {
        value.clone()
    }
}

fn compact(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut output = compact
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
}

fn split_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private TUI state transitions.
#[path = "../../tests/unit/tui.rs"]
mod tests;
