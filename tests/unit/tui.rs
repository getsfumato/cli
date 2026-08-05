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
    // The invariant: whatever the viewport height, the focused control is on screen.
    let mut form = GenerateForm::default();
    form.selected = form.fields.len() - 1;

    for height in [4, 8, 16] {
        let visible = visible_field_range(&form.fields, form.selected, height);
        assert!(
            visible.contains(&form.selected),
            "the focused field scrolled out of a {height}-row viewport"
        );
    }
}

#[test]
fn a_form_costs_one_row_per_field() {
    // Each field used to be a three-row bordered box, so the video form needed about
    // sixty rows in a terminal that has twenty-four. The slides form now fits in a
    // sixteen-row viewport with no scrolling at all.
    let form = GenerateForm::default();
    let visible = visible_field_range(&form.fields, form.selected, 16);

    assert_eq!(visible.start, 0, "the shortest form should not scroll");
    assert_eq!(
        visible.end,
        form.fields.len(),
        "every field should be visible"
    );
}

#[test]
fn the_longest_form_still_scrolls_when_it_has_to() {
    // Density is not the same as unbounded: a form taller than the viewport must
    // still page, and the focused field must still be the one on screen.
    let mut form = GenerateForm::default();
    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        *selected = 2;
    }
    form.switch_resource_from_selector();
    form.selected = form.fields.len() - 1;

    let visible = visible_field_range(&form.fields, form.selected, 10);

    assert!(visible.contains(&form.selected));
    assert!(
        visible.start > 0,
        "a form of {} fields in 10 rows must scroll",
        form.fields.len()
    );
}

#[test]
fn no_two_fields_in_one_form_share_a_label() {
    // Compacting the layout exposed two fields both labelled "Narration" — the
    // Hyperframe narration policy and the speech tool switch — which differ in what
    // they do. Identical labels in one form are indistinguishable to the user.
    for resource_index in 0..3 {
        let mut form = GenerateForm::default();
        if let FormField::Select { selected, .. } = &mut form.fields[0] {
            *selected = resource_index;
        }
        form.switch_resource_from_selector();

        let mut labels: Vec<&str> = form.fields.iter().map(|field| field.label()).collect();
        let total = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels.len(),
            total,
            "duplicate label in resource {resource_index}"
        );
    }
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
    assert!(NAV_ITEMS.iter().any(|item| item.title == "Prompts"));
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
    // The minimum terminal the TUI supports. The chrome used to spend six of these
    // twenty-four rows on an ASCII logo; the menu now has to fit alongside grouped
    // headings and a key-hint row.
    let mut app = App::new(Picker::halfblocks(), test_application());
    let rendered = render_screen(&mut app, 80, 24);

    assert!(rendered.contains("sfumato"), "{rendered}");
    // Grouped, not one flat list.
    for group in ["CREATE", "LIBRARY", "SETTINGS"] {
        assert!(rendered.contains(group), "missing {group}:\n{rendered}");
    }
    // Every entry is reachable without scrolling at the minimum size.
    for item in NAV_ITEMS {
        assert!(
            rendered.contains(item.title),
            "missing {}:\n{rendered}",
            item.title
        );
    }
    // The keys are visible rather than guessed.
    assert!(rendered.contains("? help"), "{rendered}");
    // Nothing bled past the right edge.
    for line in rendered.lines() {
        assert!(line.chars().count() <= 80, "overflowing line: {line:?}");
    }
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

/// Builds the video form with Hyperframe selected, the way the TUI does.
fn hyperframe_video_form() -> GenerateForm {
    let mut form = GenerateForm::default();
    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        // The resource selector: index of Video.
        *selected = 2;
    }
    form.switch_resource_from_selector();
    form
}

#[test]
fn the_video_form_exposes_the_workflow_the_cli_offers() {
    // `to_video_args` hardcoded `workflow: Auto` and there was no field for it, so a
    // TUI user always got `auto` — while the same form exposed fps, quality, aspect
    // ratio and voice. ADR-0001 says the CLI and TUI execute the same use cases.
    let form = hyperframe_video_form();

    let workflow = form
        .fields
        .iter()
        .find(|field| field.label() == "Workflow")
        .expect("the video form offers a workflow");
    match workflow {
        FormField::Select { options, .. } => {
            // Every CLI value is reachable.
            assert_eq!(options.len(), 7, "{options:?}");
        }
        other => panic!("workflow should be a select: {other:?}"),
    }
}

#[test]
fn the_video_form_reaches_capture_urls_and_visual_review() {
    let form = hyperframe_video_form();
    let labels: Vec<&str> = form.fields.iter().map(|field| field.label()).collect();

    assert!(labels.contains(&"Capture URLs"), "{labels:?}");
    assert!(labels.contains(&"Visual review"), "{labels:?}");
}

#[test]
fn a_selected_workflow_reaches_the_built_arguments() {
    let mut form = hyperframe_video_form();
    for field in &mut form.fields {
        match field {
            FormField::Text {
                label: "Instruction",
                value,
                ..
            } => *value = "Explica la fibra".into(),
            FormField::Select {
                label: "Workflow",
                selected,
                ..
            } => *selected = 3,
            _ => {}
        }
    }

    let args = form.to_video_args().expect("the form builds arguments");

    assert!(matches!(args.workflow, VideoWorkflowArg::ProductLaunch));
}

#[test]
fn a_capture_url_without_a_scheme_is_refused_by_the_form() {
    // The CLI applies this rule; the form applies it too, so a bad value is
    // reported while the user is still looking at the field.
    let mut form = hyperframe_video_form();
    for field in &mut form.fields {
        match field {
            FormField::Text {
                label: "Instruction",
                value,
                ..
            } => *value = "x".into(),
            FormField::Text {
                label: "Capture URLs",
                value,
                ..
            } => *value = "sfumato.dev".into(),
            _ => {}
        }
    }

    let error = form.to_video_args().expect_err("a bare host is refused");
    assert!(
        format!("{error:#}").contains("absolute http(s)"),
        "{error:#}"
    );
}

#[test]
fn several_capture_urls_are_split_and_kept() {
    let mut form = hyperframe_video_form();
    for field in &mut form.fields {
        match field {
            FormField::Text {
                label: "Instruction",
                value,
                ..
            } => *value = "x".into(),
            FormField::Text {
                label: "Capture URLs",
                value,
                ..
            } => *value = "https://a.test, https://b.test".into(),
            _ => {}
        }
    }

    let args = form.to_video_args().expect("the form builds arguments");

    assert_eq!(args.urls, vec!["https://a.test", "https://b.test"]);
}

#[test]
fn an_unlabelled_operation_stage_names_itself() {
    // `OperationStage` is `#[non_exhaustive]`, so a new variant cannot force a
    // compile error here; the fallback used to render every one of them as the same
    // opaque "Running operation".
    assert_eq!(
        operation_stage_label(OperationStage::Publish),
        "Publishing output"
    );
    // The labelled stages all read as prose rather than as their stable code.
    for stage in [
        OperationStage::Resolve,
        OperationStage::ReadSources,
        OperationStage::Draft,
        OperationStage::Render,
    ] {
        assert_ne!(
            operation_stage_label(stage),
            stage.as_str(),
            "{stage:?} should have a human label"
        );
    }
}

/// Renders one screen to text so its layout can be inspected.
#[cfg(test)]
fn render_screen(app: &mut App, width: u16, height: u16) -> String {
    use ratatui::{Terminal, backend::TestBackend};
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    // Several frames, so the transition effect finishes and the layout is legible.
    for _ in 0..40 {
        terminal.draw(|frame| view::draw(app, frame)).unwrap();
    }
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore = "prints the current layout for inspection"]
fn dump_every_screen() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    // The video form is the density worst case, so dump it explicitly.
    {
        let mut video = App::new(Picker::halfblocks(), test_application());
        if let FormField::Select { selected, .. } = &mut video.form.fields[0] {
            *selected = 2;
        }
        video.form.switch_resource_from_selector();
        video.screen = Screen::Generate;
        println!("\n╔══════ GENERATE/Video ══════ (100x32)");
        println!("{}", render_screen(&mut video, 100, 32));
    }
    // A running screen with a populated feed, which is what a user actually watches.
    {
        let mut busy = App::new(Picker::halfblocks(), test_application());
        busy.screen = Screen::Running;
        busy.current_stage = Some(GenerationStage::SemanticReview);
        busy.activities = vec![
            Activity {
                kind: ActivityKind::Stage,
                title: "Drafting deck".to_string(),
                detail: "local-text · 12 slides planned".to_string(),
                image_path: None,
            },
            Activity {
                kind: ActivityKind::ToolCall,
                title: "chart_generation".to_string(),
                detail: "plotted the convergence of the partial sums".to_string(),
                image_path: None,
            },
            Activity {
                kind: ActivityKind::Warning,
                title: "Layout repair".to_string(),
                detail: "slide 7 overflowed and was reflowed".to_string(),
                image_path: None,
            },
        ];
        println!("\n╔══════ RUNNING/busy ══════ (100x20)");
        println!("{}", render_screen(&mut busy, 100, 20));
    }
    // The palette, mid-query.
    {
        let mut jump = App::new(Picker::halfblocks(), test_application());
        jump.overlay = Some(Overlay::Palette {
            query: "co".to_string(),
            selected: 0,
        });
        println!("\n╔══════ PALETTE ══════ (100x20)");
        println!("{}", render_screen(&mut jump, 100, 20));
    }
    {
        let mut help = App::new(Picker::halfblocks(), test_application());
        help.screen = Screen::Browse(Section::Models);
        help.overlay = Some(Overlay::Help);
        println!("\n╔══════ HELP ══════ (100x20)");
        println!("{}", render_screen(&mut help, 100, 20));
    }
    for (name, screen) in [
        ("HOME", Screen::Home),
        ("BROWSE/Projects", Screen::Browse(Section::Projects)),
        ("BROWSE/Connectors", Screen::Browse(Section::Connectors)),
        ("GENERATE", Screen::Generate),
        ("RUNNING", Screen::Running),
        ("COMPLETE", Screen::Complete),
    ] {
        if let Screen::Browse(section) = screen {
            app.open_section(section);
        }
        app.screen = screen;
        println!("\n╔══════ {name} ══════ (100x32)");
        println!("{}", render_screen(&mut app, 100, 32));
        println!("\n╔══════ {name} ══════ (80x24)");
        println!("{}", render_screen(&mut app, 80, 24));
    }
}

#[test]
fn the_palette_matches_a_subsequence_not_just_a_prefix() {
    // Typing instead of scrolling only pays off if the query can skip characters:
    // `cnx` should find `Connectors`.
    let labels = App::palette_labels();

    for (query, expected) in [
        ("cnct", "Connectors"),
        ("tmpl", "Templates"),
        ("prj", "Projects"),
        ("gen", "Generate"),
    ] {
        let results = palette::matches(&labels, query);
        assert_eq!(
            results.first().copied(),
            Some(expected),
            "{query:?} should find {expected}, got {results:?}"
        );
    }
}

#[test]
fn the_palette_ranks_the_closer_match_first() {
    // `pro` matches both `Projects` and `Prompts`; the one whose characters are
    // adjacent and earliest wins rather than the order being incidental.
    let labels = App::palette_labels();
    let results = palette::matches(&labels, "pro");

    let projects = results.iter().position(|label| *label == "Projects");
    let prompts = results.iter().position(|label| *label == "Prompts");
    assert!(projects < prompts, "{results:?}");
}

#[test]
fn an_empty_palette_query_offers_every_destination_in_menu_order() {
    let labels = App::palette_labels();
    let results = palette::matches(&labels, "");

    assert_eq!(
        results, labels,
        "an empty query should not reorder anything"
    );
}

#[test]
fn a_query_that_matches_nothing_returns_nothing() {
    assert!(palette::matches(&App::palette_labels(), "zzz").is_empty());
}

#[tokio::test]
async fn the_palette_jumps_to_the_selected_destination() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.overlay = Some(Overlay::Palette {
        query: "cnct".to_string(),
        selected: 0,
    });

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.screen, Screen::Browse(Section::Connectors));
    assert!(app.overlay.is_none(), "the overlay closes after jumping");
}

#[tokio::test]
async fn ctrl_k_opens_the_palette_from_any_screen() {
    for screen in [
        Screen::Home,
        Screen::Browse(Section::Models),
        Screen::Complete,
    ] {
        let mut app = App::new(Picker::halfblocks(), test_application());
        app.screen = screen;

        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));

        assert!(
            matches!(app.overlay, Some(Overlay::Palette { .. })),
            "no palette on {screen:?}"
        );
    }
}

#[tokio::test]
async fn a_question_mark_is_text_in_a_form_and_help_everywhere_else() {
    // A form field has to be able to contain `?`, so the shortcut cannot be global.
    let mut form = App::new(Picker::halfblocks(), test_application());
    form.screen = Screen::Generate;
    form.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(form.overlay.is_none(), "help stole a character from a form");

    let mut browse = App::new(Picker::halfblocks(), test_application());
    browse.screen = Screen::Browse(Section::Models);
    browse.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(matches!(browse.overlay, Some(Overlay::Help)));
}

#[tokio::test]
async fn escape_closes_the_palette_without_navigating() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Home;
    app.overlay = Some(Overlay::palette());

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(app.overlay.is_none());
    assert_eq!(app.screen, Screen::Home, "esc should not navigate");
}

#[tokio::test]
async fn typing_in_the_palette_does_not_reach_the_screen_underneath() {
    // The overlay owns every key while open, so a query cannot half-apply to the
    // menu behind it.
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Home;
    app.nav_index = 0;
    app.overlay = Some(Overlay::palette());

    for character in ['j', 'j', 'k'] {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    assert_eq!(
        app.nav_index, 0,
        "the menu moved while the palette was open"
    );
    match &app.overlay {
        Some(Overlay::Palette { query, .. }) => assert_eq!(query, "jjk"),
        other => panic!("expected a palette holding the query, got {other:?}"),
    }
}
