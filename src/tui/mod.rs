use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

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
mod effects;
mod model;
mod reducer;
mod view;

#[cfg(test)]
use view::{select_line, stage_label, visible_field_range};

use effects::{execute_operation, load_section, spawn_connector_query};
use model::*;
use sfumato_core::{
    application::SfumatoApplication,
    config::{
        Capability, GlobalConfig, ModelDefaults, ModelOptions, ModelProfile, TextModelOptions,
    },
    config_editor::ConfigTarget,
    connectors::ConnectorPreset,
    errors::{ErrorClass, OperationStage, SfumatoError},
    operation::{
        DiscardEvents, EventSink, EventSinkError, OperationContext, OperationEvent,
        OperationEventKind,
    },
    prompts::{PromptOrigin, PromptOverrideScope},
    providers::{GenerationStage, TextGenerationEvent},
    resources::{
        pages::GeneratePageResult,
        slides::{EditSlidesResult, GenerateSlidesResult},
        videos::GenerateVideoResult,
    },
    templates::TemplateKind,
};
use tachyonfx::{EffectManager, fx};
use tokio::{
    sync::mpsc::{Receiver, Sender, channel},
    task::JoinHandle,
};
use tui_widgets::big_text::{BigText, PixelSize};

use crate::{
    cli::{
        EditSlidesArgs, GenerationToolArg, PageArgs, SlidesArgs, VideoArgs, VideoAudioArg,
        VideoEngineArg, VideoWorkflowArg,
    },
    commands::{execute_edit_slides, execute_page, execute_slides, execute_video},
};

const TICK_RATE: Duration = Duration::from_millis(80);
const NAV_ITEMS: &[(&str, &str)] = &[
    ("Generate", "Build reviewed slides, pages, or videos"),
    ("Edit", "Update an existing generated deck"),
    ("Projects", "Project working directories"),
    ("Models", "Profiles, capabilities, defaults"),
    ("Connectors", "Local and cloud model endpoints"),
    ("Themes", "Reusable visual packages"),
    ("Templates", "Reusable page and slide structures"),
    ("Artifacts", "Project logos, icons, and visuals"),
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
            terminal.draw(|frame| view::draw(app, frame))?;
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
            BrowseAction::ConnectorSetup,
            BrowseAction::ConnectorModels,
            BrowseAction::ConnectorStatus,
        ],
        Section::Themes => &[
            BrowseAction::ThemeCreate,
            BrowseAction::ThemeImport,
            BrowseAction::ThemeExport,
            BrowseAction::ThemeUse,
        ],
        Section::Templates => &[BrowseAction::TemplateCreate],
        Section::Artifacts => &[BrowseAction::ArtifactAdd, BrowseAction::ArtifactRemove],
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
