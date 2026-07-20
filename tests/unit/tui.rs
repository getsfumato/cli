use super::*;
use clap::Parser;

use crate::cli::SlidesArgs;

#[derive(Parser)]
struct SlidesCliHarness {
    #[command(flatten)]
    args: SlidesArgs,
}

fn test_application() -> Arc<SfumatoApplication> {
    Arc::new(sfumato_adapters::application::production_application().unwrap())
}

#[test]
fn generation_form_requires_an_instruction() {
    let form = GenerateForm::default();

    assert!(form.to_slides_args().is_err());
}

#[test]
fn generation_form_builds_capability_override_and_sources() {
    let mut form = GenerateForm::default();
    for field in &mut form.fields {
        match field {
            FormField::Text {
                label: "Instruction",
                value,
                ..
            } => *value = "Teach transforms".into(),
            FormField::Text {
                label: "Sources",
                value,
                ..
            } => *value = "notes, examples".into(),
            FormField::Text {
                label: "Text model",
                value,
                ..
            } => *value = "cloud-draft".into(),
            _ => {}
        }
    }

    let args = form.to_slides_args().unwrap();

    assert_eq!(
        args.inputs,
        vec![PathBuf::from("notes"), PathBuf::from("examples")]
    );
    assert_eq!(args.model_overrides, vec!["text=cloud-draft"]);
}

#[test]
fn generation_form_builds_a_page_with_multiple_catalog_plugins() {
    let mut form = GenerateForm::with_plugins(
        vec!["shadcn".into()],
        vec!["lottie".into(), "motion".into(), "threejs".into()],
    );
    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        *selected = 1;
    }
    form.switch_resource_from_selector();
    for field in &mut form.fields {
        match field {
            FormField::Text {
                label: "Instruction",
                value,
                ..
            } => *value = "Build an interactive transform explorer".into(),
            FormField::MultiSelect {
                label: "Utility plugins",
                selected,
                ..
            } => {
                selected.insert(1);
                selected.insert(2);
            }
            _ => {}
        }
    }

    assert_eq!(form.resource, GenerateResource::Page);
    let args = form.to_page_args().unwrap();
    assert_eq!(args.plugins, vec!["motion", "threejs"]);
}

#[test]
fn generation_form_uses_resource_specific_publication_and_controls() {
    let mut form = GenerateForm::with_plugins(vec!["shadcn".into()], vec!["motion".into()]);
    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "Publish PDF")
    );
    assert!(
        !form
            .fields
            .iter()
            .any(|field| field.label() == "UI library")
    );

    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        *selected = 1;
    }
    form.switch_resource_from_selector();
    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "Publish page")
    );
    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "UI library")
    );
    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "Video generation")
    );
    assert!(
        !form
            .fields
            .iter()
            .any(|field| field.label() == "Publish PDF")
    );

    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        *selected = 2;
    }
    form.switch_resource_from_selector();
    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "Publish MP4")
    );
    assert!(
        !form
            .fields
            .iter()
            .any(|field| field.label() == "UI library")
    );
    assert!(
        !form
            .fields
            .iter()
            .any(|field| field.label() == "Video generation")
    );
}

#[test]
fn video_engine_switches_only_show_applicable_fields() {
    let mut form = GenerateForm::default();
    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        *selected = 2;
    }
    form.switch_resource_from_selector();

    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "Code model")
    );
    assert!(form.fields.iter().any(|field| field.label() == "FPS"));
    assert!(!form.fields.iter().any(|field| field.label() == "Audio"));

    let engine = form
        .field_ids
        .iter()
        .position(|id| *id == GenerateFieldId::Engine)
        .unwrap();
    if let FormField::Select { selected, .. } = &mut form.fields[engine] {
        *selected = 2;
    }
    form.switch_video_engine_from_selector();

    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "Video model")
    );
    assert!(form.fields.iter().any(|field| field.label() == "Audio"));
    assert!(
        !form
            .fields
            .iter()
            .any(|field| field.label() == "Code model")
    );
    assert!(!form.fields.iter().any(|field| field.label() == "FPS"));
}

#[test]
fn cli_and_tui_build_equivalent_generation_arguments() {
    let cli_args = SlidesCliHarness::try_parse_from([
        "slides",
        "notes",
        "examples",
        "--instruction",
        "Teach transforms",
        "--project",
        "university",
        "--theme",
        "gruvbox",
        "--model",
        "text=cloud-draft",
        "--review-model",
        "local-review",
        "--out",
        "Presentations",
    ])
    .unwrap()
    .args;

    let mut form = GenerateForm::default();
    for field in &mut form.fields {
        if let FormField::Text { label, value, .. } = field {
            *value = match *label {
                "Instruction" => "Teach transforms",
                "Project" => "university",
                "Sources" => "notes, examples",
                "Theme" => "gruvbox",
                "Publish PDF" => "Presentations",
                "Text model" => "cloud-draft",
                "Reviewer" => "local-review",
                _ => continue,
            }
            .to_string();
        }
    }
    let tui_args = form.to_slides_args().unwrap();

    assert_eq!(tui_args.inputs, cli_args.inputs);
    assert_eq!(tui_args.instruction, cli_args.instruction);
    assert_eq!(tui_args.project, cli_args.project);
    assert_eq!(tui_args.theme, cli_args.theme);
    assert_eq!(tui_args.out, cli_args.out);
    assert_eq!(tui_args.model_overrides, cli_args.model_overrides);
    assert_eq!(tui_args.review_model, cli_args.review_model);
    assert_eq!(tui_args.no_review, cli_args.no_review);
}

#[test]
fn image_tool_result_is_human_readable_and_exposes_preview_path() {
    let (summary, path) = format_tool_result(
        "sfumato_image_gen",
        r#"{"path":"/tmp/image.png","markdown_path":"images/image.png","model_profile":"image-model"}"#,
    );

    assert!(summary.contains("Created images/image.png"));
    assert_eq!(path, Some(PathBuf::from("/tmp/image.png")));
    assert!(!summary.contains("markdown_path"));
}

#[test]
fn directory_tool_result_does_not_leak_json() {
    let (summary, _) = format_tool_result(
        "sfumato_list_directory",
        r#"{"path":"/tmp/notes","entries":[{"name":"one.md","kind":"file"},{"name":"week-1","kind":"directory"}]}"#,
    );

    assert!(summary.contains("2 entries"));
    assert!(summary.contains("1 files"));
    assert!(!summary.contains("\"entries\""));
}

#[test]
fn stage_labels_include_generation_and_editing() {
    assert_eq!(stage_label(GenerationStage::Draft), "Draft");
    assert_eq!(stage_label(GenerationStage::Edit), "Content edit");
}

#[test]
fn context_compaction_event_has_a_readable_activity() {
    let activity = Activity::from_event(&TextGenerationEvent::ContextCompactionStarted {
        stage: GenerationStage::SemanticReview,
        original_chars: 91_000,
        compacted_chars: 24_000,
    });

    assert_eq!(activity.title, "Compacting model context");
    assert!(activity.detail.contains("reviewing content"));
    assert!(activity.detail.contains("91000 to 24000 characters"));
}

#[test]
fn compact_viewport_keeps_the_selected_form_control_visible() {
    let mut form = GenerateForm::default();
    form.selected = form.fields.len() - 1;

    let visible = visible_field_range(&form.fields, form.selected, 16);

    assert!(visible.contains(&form.selected));
    assert!(visible.start > 0);
}

#[test]
fn home_enter_opens_the_selected_resource_form() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.nav_index = 0;

    app.handle_home_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.screen, Screen::Generate);
}

#[test]
fn main_menu_exposes_prompt_management() {
    assert!(NAV_ITEMS.iter().any(|(name, _)| *name == "Prompts"));
    assert_eq!(
        section_actions(Section::Prompts),
        &[
            BrowseAction::PromptCustomizeUser,
            BrowseAction::PromptCustomizeProject,
            BrowseAction::PromptValidate,
        ]
    );
}

#[test]
fn connector_browser_exposes_native_catalog_and_status_actions() {
    assert!(section_actions(Section::Connectors).contains(&BrowseAction::ConnectorModels));
    assert!(section_actions(Section::Connectors).contains(&BrowseAction::ConnectorStatus));
}

#[test]
fn browse_arrow_keys_move_rows_without_requiring_tab_first() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Browse(Section::Models);
    app.browse_focus = BrowseFocus::Actions;
    app.browse_rows = vec![
        BrowseRow {
            title: "one".to_string(),
            subtitle: String::new(),
            detail: String::new(),
            active: false,
        },
        BrowseRow {
            title: "two".to_string(),
            subtitle: String::new(),
            detail: String::new(),
            active: false,
        },
    ];

    app.handle_browse_key(
        Section::Models,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    );

    assert_eq!(app.browse_focus, BrowseFocus::Rows);
    assert_eq!(app.browse_index, 1);
}

#[test]
fn browse_horizontal_keys_select_actions_and_vertical_keys_return_to_rows() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Browse(Section::Models);
    app.browse_rows = vec![BrowseRow {
        title: "codex".to_string(),
        subtitle: String::new(),
        detail: String::new(),
        active: false,
    }];
    app.browse_focus = BrowseFocus::Rows;

    app.handle_browse_key(
        Section::Models,
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
    );
    assert_eq!(app.browse_focus, BrowseFocus::Actions);

    app.handle_browse_key(
        Section::Models,
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
    );
    assert_eq!(app.browse_focus, BrowseFocus::Rows);
}

#[test]
fn edit_form_builds_a_focused_slide_command() {
    let mut form = EditForm::default();
    for field in &mut form.fields {
        match field {
            FormField::Text {
                label: "Deck",
                value,
                ..
            } => *value = "/tmp/deck.md".into(),
            FormField::Text {
                label: "Instruction",
                value,
                ..
            } => *value = "Clarify slide two".into(),
            FormField::Text {
                label: "Text model",
                value,
                ..
            } => *value = "cloud-draft".into(),
            _ => {}
        }
    }

    let args = form.to_args().unwrap();

    assert_eq!(args.markdown_path, PathBuf::from("/tmp/deck.md"));
    assert_eq!(args.instruction, "Clarify slide two");
    assert_eq!(args.model_overrides, vec!["text=cloud-draft"]);
}

#[test]
fn dashboard_renders_at_eighty_by_twenty_four() {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new(Picker::halfblocks(), test_application());

    terminal.draw(|frame| view::draw(&mut app, frame)).unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("SFUMATO"));
    assert!(rendered.contains("WORKSPACE"));
    assert!(rendered.contains("ACTIVE PROJECT"));
}
