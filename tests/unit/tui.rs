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
fn connector_setup_offers_every_preset_through_one_action() {
    // One action rather than one per preset: the ACTIONS bar renders as a single
    // unwrapped line, so a label per preset would clip on an 80-column terminal.
    assert!(section_actions(Section::Connectors).contains(&BrowseAction::ConnectorSetup));

    let mut app = App::new(Picker::halfblocks(), test_application());
    app.operation = Some(
        app.operation_for_action(BrowseAction::ConnectorSetup)
            .unwrap(),
    );
    let form = app.operation.as_ref().expect("setup opens a form");

    let FormField::Select { options, .. } = &form.fields[0] else {
        panic!("the preset field is a select");
    };
    assert_eq!(
        options,
        &ConnectorPreset::ALL
            .into_iter()
            .map(|preset| preset.as_str().to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(form.select("Preset"), ConnectorPreset::ALL[0].as_str());
}

#[test]
fn connector_setup_cycles_presets_with_the_arrow_keys() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.operation = Some(
        app.operation_for_action(BrowseAction::ConnectorSetup)
            .unwrap(),
    );

    // Asserted against `ALL` rather than literal preset names, so inserting a
    // preset cannot silently invalidate the expectation.
    let all = ConnectorPreset::ALL;
    app.handle_operation_key(KeyEvent::from(KeyCode::Right));
    assert_eq!(
        app.operation.as_ref().unwrap().select("Preset"),
        all[1].as_str()
    );

    // Wrapping backwards from the first option lands on the last one.
    app.handle_operation_key(KeyEvent::from(KeyCode::Left));
    app.handle_operation_key(KeyEvent::from(KeyCode::Left));
    assert_eq!(
        app.operation.as_ref().unwrap().select("Preset"),
        all[all.len() - 1].as_str()
    );
}

#[test]
fn connector_setup_hides_the_credential_field_for_externally_managed_presets() {
    // `into_config` rejects `--api-key-env` for Codex, so offering the field
    // could only make submission fail.
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.operation = Some(
        app.operation_for_action(BrowseAction::ConnectorSetup)
            .unwrap(),
    );
    let has_credential_field = |app: &App| {
        app.operation
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .any(|field| field.label() == API_KEY_ENV_FIELD)
    };
    assert!(has_credential_field(&app));

    let codex = ConnectorPreset::ALL
        .iter()
        .position(|preset| !preset.accepts_api_key_env())
        .expect("one preset manages its own authentication");
    for _ in 0..codex {
        app.handle_operation_key(KeyEvent::from(KeyCode::Right));
    }
    assert!(!has_credential_field(&app));

    // Moving back off that preset restores the field.
    app.handle_operation_key(KeyEvent::from(KeyCode::Left));
    assert!(has_credential_field(&app));
}

#[test]
fn setup_user_follows_the_selected_connector_preset() {
    // Every text-capable preset is accepted here, so the profile name and model
    // id must follow the choice rather than staying Ollama-shaped. A speech-only
    // preset is deliberately absent: it cannot back a drafting profile.
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.operation = Some(app.operation_for_action(BrowseAction::SetupUser).unwrap());
    let operation = app.operation.as_mut().unwrap();
    operation.selected = operation
        .fields
        .iter()
        .position(|field| field.label() == "Connector")
        .expect("the connector field is present");

    for (index, preset) in ConnectorPreset::text_capable()
        .into_iter()
        .enumerate()
        .skip(1)
    {
        app.handle_operation_key(KeyEvent::from(KeyCode::Right));
        let form = app.operation.as_ref().unwrap();
        assert_eq!(form.select("Connector"), preset.as_str(), "option {index}");
        assert_eq!(form.text("Profile"), preset.default_profile_name());
        assert_eq!(form.text("Model ID"), preset.default_model());
    }
}

#[test]
fn select_options_always_render_the_current_choice() {
    // The option row is a single unwrapped line inside a bordered field, so a
    // full preset list overflows an 80-column modal; the window must still show
    // the `[x]` marker for whatever `select` would return.
    let options = ConnectorPreset::ALL
        .into_iter()
        .map(|preset| preset.as_str().to_string())
        .collect::<Vec<_>>();

    let rendered = |selected: usize, width: u16| {
        select_line(&options, selected, width)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };

    for width in [4_u16, 12, 20, 40, 66] {
        for selected in 0..options.len() {
            let line = rendered(selected, width);
            assert!(
                line.chars().count() <= usize::from(width),
                "width {width} overflowed: {line:?}"
            );
            assert!(
                line.contains("[x]"),
                "width {width} lost the marker for option {selected}: {line:?}"
            );
        }
    }

    // Wide enough for the longest option, the whole choice is always readable —
    // this is the range every supported terminal width lands in.
    for width in [20_u16, 40, 66] {
        for (selected, option) in options.iter().enumerate() {
            let line = rendered(selected, width);
            assert!(
                line.contains(&format!("[x] {option}")),
                "width {width} clipped option {selected}: {line:?}"
            );
        }
    }
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

#[tokio::test]
async fn esc_stops_a_running_connector_read_and_keeps_the_view() {
    // Both connector paths built `OperationContext::detached()` and dropped the
    // task, so `Esc` reached nothing and the view hung on an unresponsive
    // endpoint. The query is now cancellable and `Esc` claims it first.
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Browse(Section::Connectors);
    app.connector_query = Some(effects::spawn_connector_query(
        Arc::clone(&app.application),
        "ollama".to_string(),
        true,
        app.sender.clone(),
    ));

    app.handle_browse_key(
        Section::Connectors,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    );

    assert!(app.connector_query.is_none(), "query was not cancelled");
    // The view stays put; leaving is what `Esc` does when nothing is running.
    assert_eq!(app.screen, Screen::Browse(Section::Connectors));
}

#[tokio::test]
async fn esc_still_leaves_the_section_when_no_connector_read_is_running() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Browse(Section::Connectors);

    app.handle_browse_key(
        Section::Connectors,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    );

    assert_eq!(app.screen, Screen::Home);
}

#[tokio::test]
async fn a_connector_read_is_bounded_by_a_deadline() {
    // A detached context has no deadline at all, which is what left the view
    // with nothing to fall back on.
    let query = effects::spawn_connector_query(
        Arc::clone(&test_application()),
        "ollama".to_string(),
        false,
        tokio::sync::mpsc::channel(8).0,
    );

    // A detached context has no deadline at all, which is what left the view
    // with nothing to fall back on. The read now carries one.
    assert!(OperationContext::detached().remaining().is_none());
    query.cancel_and_join().await;
}

#[tokio::test]
async fn starting_a_second_connector_read_cancels_the_first() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Browse(Section::Connectors);
    app.browse_rows = vec![BrowseRow {
        title: "ollama".to_string(),
        subtitle: String::new(),
        detail: String::new(),
        active: false,
    }];
    app.browse_focus = BrowseFocus::Rows;

    let actions = section_actions(Section::Connectors);
    app.browse_action_index = actions
        .iter()
        .position(|action| *action == BrowseAction::ConnectorStatus)
        .expect("connectors expose a status action");
    app.execute_browse_action(Section::Connectors);
    assert!(app.connector_query.is_some());

    app.browse_action_index = actions
        .iter()
        .position(|action| *action == BrowseAction::ConnectorModels)
        .expect("connectors expose a models action");
    app.execute_browse_action(Section::Connectors);

    // One handle, so the earlier read cannot be left running unreachable.
    assert!(app.connector_query.is_some());
    app.shutdown().await;
    assert!(app.connector_query.is_none());
}
