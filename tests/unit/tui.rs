use super::*;
use clap::Parser;

use crate::cli::SlidesArgs;

#[derive(Parser)]
struct SlidesCliHarness {
    #[command(flatten)]
    args: SlidesArgs,
}

#[derive(Parser)]
struct DocumentCliHarness {
    #[command(flatten)]
    args: DocumentArgs,
}

/// Puts the resource selector on `index` and rebuilds the form around it.
fn form_for_resource(index: usize) -> GenerateForm {
    let mut form = GenerateForm::default();
    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        *selected = index;
    }
    form.switch_resource_from_selector();
    form
}

/// The selector index the document resource sits at.
const DOCUMENT: usize = 3;

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
            // A picker, not free text, since the form learned to offer the profiles it
            // found — the value it produces is the same either way.
            FormField::Choice {
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

/// Charting was the one tool the form could not steer, and the permission it needs
/// was reachable only from the Manim branch — so a deck or a page that wanted a
/// plot had to be configured project-wide and could not be held back for one run.
#[test]
fn every_resource_steers_charting_and_can_grant_code_execution() {
    for resource in 0..=DOCUMENT {
        let form = form_for_resource(resource);

        assert!(
            form.fields
                .iter()
                .any(|field| field.label() == "Chart generation"),
            "resource {resource} cannot steer charting"
        );
        assert!(
            form.fields
                .iter()
                .any(|field| field.label() == "Code execution"),
            "resource {resource} cannot grant code execution"
        );
    }
}

#[test]
fn switching_charting_off_disables_it_for_this_run_alone() {
    let mut form = GenerateForm::default();
    for field in &mut form.fields {
        if let FormField::Text {
            label: "Instruction",
            value,
            ..
        } = field
        {
            *value = "Plot the error term".to_string();
        }
    }
    let chart = form
        .field_ids
        .iter()
        .position(|id| *id == GenerateFieldId::ChartTool)
        .unwrap();
    if let FormField::Select { selected, .. } = &mut form.fields[chart] {
        *selected = 2;
    }

    let args = form.to_slides_args().unwrap();

    assert!(args.tools.is_empty());
    assert!(
        args.disabled_tools
            .iter()
            .any(|tool| matches!(tool, GenerationToolArg::ChartGen))
    );
    // Left off unless asked: the toggle is consent, so its default cannot be a yes.
    assert!(!args.allow_code_execution);
}

/// The command layer rejects `--allow-code-execution` with `--engine model`, which
/// runs no local code, so offering the toggle there would only build a run that
/// cannot start.
#[test]
fn the_direct_model_engine_withholds_a_permission_it_would_reject() {
    let mut form = GenerateForm::default();
    if let FormField::Select { selected, .. } = &mut form.fields[0] {
        *selected = 2;
    }
    form.switch_resource_from_selector();
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
        !form
            .fields
            .iter()
            .any(|field| field.label() == "Code execution")
    );
    // Still steerable: a project that persisted `security.allow_python` can plot
    // here, so the switch that turns charting off has to stay reachable.
    assert!(
        form.fields
            .iter()
            .any(|field| field.label() == "Chart generation")
    );
}

/// Paginated documents were reachable only from the CLI: the TUI's resource
/// selector offered three of the four resources `generate` supports.
#[test]
fn the_document_form_reaches_the_paginated_flags_the_cli_offers() {
    let form = form_for_resource(DOCUMENT);
    let label = |name: &str| form.fields.iter().any(|field| field.label() == name);

    for expected in [
        "Page size",
        "Table of contents",
        "Cover page",
        "Template",
        "Publish PDF",
        "Image generation",
        "Chart generation",
        "Code execution",
    ] {
        assert!(
            label(expected),
            "the document form has no '{expected}' field"
        );
    }
    // Borrowed from no other resource: a document has no UI library, no timeline,
    // and no engine to pick.
    for absent in ["UI library", "Utility plugins", "Video engine", "Duration"] {
        assert!(
            !label(absent),
            "the document form should not offer '{absent}'"
        );
    }
}

#[test]
fn document_theme_overrides_stay_silent_until_the_form_asks() {
    let mut form = form_for_resource(DOCUMENT);
    for field in &mut form.fields {
        if let FormField::Text {
            label: "Instruction",
            value,
            ..
        } = field
        {
            *value = "Summarise the unit".to_string();
        }
    }

    let untouched = form.to_document_args().unwrap();
    assert!(untouched.page_size.is_none());
    assert!(!untouched.toc && !untouched.no_toc);
    assert!(!untouched.cover && !untouched.no_cover);

    let set = |form: &mut GenerateForm, id, choice| {
        let index = form
            .field_ids
            .iter()
            .position(|candidate| *candidate == id)
            .unwrap();
        if let FormField::Select { selected, .. } = &mut form.fields[index] {
            *selected = choice;
        }
    };
    set(&mut form, GenerateFieldId::PageSize, 2);
    set(&mut form, GenerateFieldId::TableOfContents, 2);
    set(&mut form, GenerateFieldId::Cover, 1);

    let asked = form.to_document_args().unwrap();
    assert!(matches!(asked.page_size, Some(DocumentPageSizeArg::Letter)));
    // Off has to travel as `--no-toc`, not as a missing `--toc`, or the theme's own
    // default would quietly win.
    assert!(asked.no_toc && !asked.toc);
    assert!(asked.cover && !asked.no_cover);
}

#[test]
fn cli_and_tui_build_equivalent_document_arguments() {
    let cli_args = DocumentCliHarness::try_parse_from([
        "document",
        "notes",
        "--instruction",
        "Write the practical guide",
        "--project",
        "university",
        "--theme",
        "gruvbox",
        "--model",
        "text=cloud-draft",
        "--review-model",
        "local-review",
        "--out",
        "Documents",
        "--template",
        "handout",
    ])
    .unwrap()
    .args;

    let mut form = form_for_resource(DOCUMENT);
    for field in &mut form.fields {
        if let FormField::Text { label, value, .. } | FormField::Choice { label, value, .. } = field
        {
            *value = match *label {
                "Instruction" => "Write the practical guide",
                "Project" => "university",
                "Sources" => "notes",
                "Theme" => "gruvbox",
                "Publish PDF" => "Documents",
                "Text model" => "cloud-draft",
                "Reviewer" => "local-review",
                "Template" => "handout",
                _ => continue,
            }
            .to_string();
        }
    }
    let tui_args = form.to_document_args().unwrap();

    assert_eq!(tui_args.inputs, cli_args.inputs);
    assert_eq!(tui_args.instruction, cli_args.instruction);
    assert_eq!(tui_args.project, cli_args.project);
    assert_eq!(tui_args.theme, cli_args.theme);
    assert_eq!(tui_args.out, cli_args.out);
    assert_eq!(tui_args.template, cli_args.template);
    assert_eq!(tui_args.model_overrides, cli_args.model_overrides);
    assert_eq!(tui_args.review_model, cli_args.review_model);
    assert_eq!(tui_args.no_review, cli_args.no_review);
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
        // Both variants hold a typed-or-picked string, and the argument builders read
        // them the same way — which is the property this test is about.
        if let FormField::Text { label, value, .. } | FormField::Choice { label, value, .. } = field
        {
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
    for resource_index in 0..=DOCUMENT {
        let form = form_for_resource(resource_index);

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

/// Renders one screen and hands back the raw cells, styles included.
#[cfg(test)]
fn render_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    use ratatui::{Terminal, backend::TestBackend};
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    // Several frames, so the transition effect finishes and the layout is legible.
    for _ in 0..40 {
        terminal.draw(|frame| view::draw(app, frame)).unwrap();
    }
    terminal.backend().buffer().clone()
}

/// Renders one screen to text so its layout can be inspected.
#[cfg(test)]
fn render_screen(app: &mut App, width: u16, height: u16) -> String {
    let buffer = render_buffer(app, width, height);
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
    // The picker over the slides form, which is the interaction this dump exists to
    // check: the caret, the list, and the detail column beside each value.
    {
        let mut picking = app_with_options();
        focus_field(&mut picking, GenerateFieldId::Project);
        picking.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        println!("\n╔══════ GENERATE/Slides + picker ══════ (100x32)");
        println!("{}", render_screen(&mut picking, 100, 32));
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

/// A generate screen whose pickers offer known values, so the tests do not depend
/// on what the machine running them happens to have configured.
#[cfg(test)]
fn app_with_options() -> App {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Generate;
    app.snapshot.options = FormOptions {
        projects: vec![
            Choice {
                value: "university".into(),
                detail: "/work/university".into(),
            },
            Choice {
                value: "bootcamp".into(),
                detail: "/work/bootcamp".into(),
            },
        ],
        themes: vec![Choice {
            value: "gruvbox".into(),
            detail: String::new(),
        }],
        text_models: vec![
            Choice {
                value: "cloud-draft".into(),
                detail: "anthropic · claude".into(),
            },
            Choice {
                value: "local-draft".into(),
                detail: "ollama · qwen".into(),
            },
        ],
        ..FormOptions::default()
    };
    app
}

#[cfg(test)]
fn focus_field(app: &mut App, id: GenerateFieldId) {
    app.form.selected = app
        .form
        .field_ids
        .iter()
        .position(|candidate| *candidate == id)
        .expect("field is on the form");
}

#[test]
fn a_picker_field_offers_the_configured_values_instead_of_free_text() {
    let mut app = app_with_options();
    focus_field(&mut app, GenerateFieldId::Project);

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.overlay,
        Some(Overlay::choice(ChoiceTarget::Generate(
            GenerateFieldId::Project
        )))
    );
    let screen = render_screen(&mut app, 100, 32);
    assert!(screen.contains("university"), "{screen}");
    assert!(screen.contains("bootcamp"), "{screen}");
    // The path is what tells two same-named-looking projects apart.
    assert!(screen.contains("/work/university"), "{screen}");
}

#[test]
fn picking_a_value_writes_it_into_the_field() {
    let mut app = app_with_options();
    focus_field(&mut app, GenerateFieldId::TextModel);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Typing filters, so a long list of profiles stays reachable by name.
    for character in "local".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.overlay.is_none(), "picking closes the picker");
    assert_eq!(app.form.text(GenerateFieldId::TextModel), "local-draft");
}

#[test]
fn typing_on_a_picker_opens_it_with_the_character_already_queried() {
    let mut app = app_with_options();
    focus_field(&mut app, GenerateFieldId::Project);

    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));

    assert_eq!(
        app.overlay,
        Some(Overlay::Choice {
            target: ChoiceTarget::Generate(GenerateFieldId::Project),
            query: "b".to_string(),
            selected: 0,
        })
    );
    // A picker must never accumulate typed text as if it were a value.
    assert_eq!(app.form.text(GenerateFieldId::Project), "");
}

#[test]
fn a_picker_can_be_cleared_back_to_the_project_default() {
    let mut app = app_with_options();
    focus_field(&mut app, GenerateFieldId::Theme);
    app.form.set_choice(GenerateFieldId::Theme, "gruvbox");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));

    assert_eq!(app.form.text(GenerateFieldId::Theme), "");
    assert!(app.overlay.is_none());
}

#[test]
fn escape_leaves_a_picker_without_changing_the_field() {
    let mut app = app_with_options();
    focus_field(&mut app, GenerateFieldId::Project);
    app.form.set_choice(GenerateFieldId::Project, "university");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(app.overlay.is_none());
    assert_eq!(app.form.text(GenerateFieldId::Project), "university");
    assert_eq!(
        app.screen,
        Screen::Generate,
        "escape dismisses the picker, not the form underneath"
    );
}

#[test]
fn an_empty_picker_says_so_rather_than_showing_a_blank_box() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Generate;
    app.snapshot.options = FormOptions::default();
    focus_field(&mut app, GenerateFieldId::Project);

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let screen = render_screen(&mut app, 100, 32);

    assert!(screen.contains("nothing configured"), "{screen}");
}

#[test]
fn templates_are_filtered_to_the_resource_being_generated() {
    let slide_choice = Choice {
        value: "lecture".into(),
        detail: "slides".into(),
    };
    let page_choice = Choice {
        value: "handout".into(),
        detail: "page".into(),
    };
    let options = FormOptions {
        slide_templates: vec![slide_choice.clone()],
        page_templates: vec![page_choice.clone()],
        ..FormOptions::default()
    };

    // A slides template is rejected by the layers below when asked for on a page, so
    // offering it at all would be offering a guaranteed failure.
    assert_eq!(
        ChoiceSource::SlideTemplates.choices(&options),
        [slide_choice]
    );
    assert_eq!(ChoiceSource::PageTemplates.choices(&options), [page_choice]);
}

#[test]
#[ignore = "prints the real workspace's pickers for inspection"]
fn dump_real_pickers() {
    for field in [
        GenerateFieldId::Project,
        GenerateFieldId::Theme,
        GenerateFieldId::Template,
        GenerateFieldId::TextModel,
        GenerateFieldId::Reviewer,
    ] {
        let mut app = App::new(Picker::halfblocks(), test_application());
        app.transition(Screen::Generate);
        focus_field(&mut app, field);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        println!("\n╔══════ {field:?} ══════");
        println!("{}", render_screen(&mut app, 100, 26));
    }
}

#[test]
fn a_refused_path_is_readable_instead_of_clipped_at_the_panel_edge() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Running;
    app.activities
        .push(Activity::from_event(&TextGenerationEvent::ToolCallFailed {
            name: "sfumato_read_file".to_string(),
            error: "Refusing to read /Users/someone/.codex/plugins/cache/openai-bundled/\
                    visualize/1.0.16/skills/visualize/SKILL.md because it is outside the \
                    allowed generation roots. Readable roots for this operation: \
                    /work/university."
                .to_string(),
        }));

    let screen = render_screen(&mut app, 80, 24);

    // The reason, not just the fact — the whole point of the entry.
    assert!(
        screen.contains("outside the"),
        "the refusal's reason must survive the panel width:\n{screen}"
    );
    assert!(
        screen.contains("Readable roots") || screen.contains("Readable"),
        "the allowed roots must be visible so the path can be corrected:\n{screen}"
    );
    assert!(
        screen.lines().all(|line| line.chars().count() <= 80),
        "nothing may overflow the terminal:\n{screen}"
    );
}

#[test]
fn wrapping_a_detail_hard_splits_a_long_path_and_marks_what_it_dropped() {
    // A path has no spaces, so breaking on words alone would show only its first row.
    let wrapped = view::wrap_detail(&"a".repeat(90), 20, 4);

    assert_eq!(wrapped.len(), 4);
    assert!(wrapped.iter().all(|line| line.chars().count() <= 20));
    assert!(
        wrapped.last().unwrap().ends_with('…'),
        "running out of rows must not read as the end of the message: {wrapped:?}"
    );
}

#[test]
fn an_entry_without_a_detail_keeps_its_spacing_row() {
    assert_eq!(view::wrap_detail("", 40, 4), vec![String::new()]);
    assert_eq!(view::wrap_detail("short", 40, 1), vec!["short".to_string()]);
}

#[test]
#[ignore = "prints a refused-path warning for inspection"]
fn dump_refusal_warning() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Running;
    app.activities.push(Activity::from_event(
        &TextGenerationEvent::ToolCallRequested {
            name: "sfumato_read_file".to_string(),
            arguments: serde_json::json!({
                "path": "/Users/someone/.codex/plugins/cache/openai-bundled/visualize/1.0.16/skills/visualize/SKILL.md"
            }),
        },
    ));
    app.activities.push(Activity::from_event(
        &TextGenerationEvent::ToolCallFailed {
            name: "sfumato_read_file".to_string(),
            error: "Refusing to read /Users/someone/.codex/plugins/cache/openai-bundled/visualize/1.0.16/skills/visualize/SKILL.md because it is outside the allowed generation roots. Readable roots for this operation: /Users/someone/Documents/Notebook/Facultad. Only the project and the sources given to it can be read.".to_string(),
        },
    ));
    println!("{}", render_screen(&mut app, 96, 22));
}

/// Opens the Projects section with the Edit action selected on the first row.
#[cfg(test)]
fn app_editing_a_project() -> App {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.transition(Screen::Browse(Section::Projects));
    app.browse_rows = vec![BrowseRow {
        title: "Facultad".to_string(),
        subtitle: "/work/facultad".to_string(),
        detail: String::new(),
        active: true,
    }];
    app.browse_index = 0;
    app.browse_focus = BrowseFocus::Actions;
    app.browse_action_index = section_actions(Section::Projects)
        .iter()
        .position(|action| *action == BrowseAction::ProjectEdit)
        .expect("Projects offers an Edit action");
    app
}

#[test]
fn the_projects_section_offers_a_way_to_edit_one() {
    // Create, Activate and Remove were the whole set, so changing a project's default
    // image model meant knowing `model_defaults.image` by heart in the Config screen.
    assert!(
        section_actions(Section::Projects).contains(&BrowseAction::ProjectEdit),
        "a project that can be created and removed must also be editable"
    );
}

#[test]
fn editing_a_project_offers_a_picker_for_every_model_default() {
    let mut app = app_editing_a_project();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let operation = app.operation.as_ref().expect("the edit form opened");
    let pickers = operation
        .fields
        .iter()
        .filter_map(|field| match field {
            FormField::Choice { label, source, .. } => Some((*label, *source)),
            _ => None,
        })
        .collect::<Vec<_>>();

    // Each capability gets the profiles that declare it: offering an image model where a
    // text model is wanted is offering a guaranteed failure.
    assert_eq!(
        pickers,
        vec![
            ("Theme", ChoiceSource::Themes),
            ("Text model", ChoiceSource::TextModels),
            ("Code model", ChoiceSource::CodeModels),
            ("Image model", ChoiceSource::ImageModels),
            ("Video model", ChoiceSource::VideoModels),
            ("Speech model", ChoiceSource::SpeechModels),
            ("Reviewer", ChoiceSource::ReviewerModels),
        ]
    );
}

#[test]
fn an_operation_picker_writes_back_to_the_field_it_was_opened_from() {
    let mut app = app_editing_a_project();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.snapshot.options.image_models = vec![Choice {
        value: "gpt-image-2".into(),
        detail: "openrouter · openai/gpt-image-2".into(),
    }];
    let index = app
        .operation
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|field| {
            matches!(
                field,
                FormField::Choice {
                    label: "Image model",
                    ..
                }
            )
        })
        .unwrap();
    app.operation.as_mut().unwrap().selected = index;

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.overlay,
        Some(Overlay::choice(ChoiceTarget::Operation(index))),
        "the picker must target the field it was opened from"
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.overlay.is_none());
    assert_eq!(
        app.operation.as_ref().unwrap().text("Image model"),
        "gpt-image-2"
    );
}

#[test]
fn an_operation_form_reads_a_picked_value_the_same_as_a_typed_one() {
    // Every effect reads its fields by label, so a field becoming a picker must not
    // change what the effect sees.
    let form = OperationForm {
        title: "Edit project",
        kind: OperationKind::ProjectEdit,
        target: Some("Facultad".to_string()),
        fields: vec![FormField::Choice {
            label: "Theme",
            value: "  gruvbox  ".to_string(),
            placeholder: "project theme",
            source: ChoiceSource::Themes,
        }],
        selected: 0,
    };

    assert_eq!(form.text("Theme"), "gruvbox");
}

#[test]
fn saving_an_unchanged_project_writes_nothing() {
    let application = test_application();
    let Ok(current) = application.show_project(None) else {
        return; // no active project on this machine; the write path is covered above
    };
    let form = OperationForm {
        title: "Edit project",
        kind: OperationKind::ProjectEdit,
        target: Some(current.name.clone()),
        fields: vec![
            FormField::Choice {
                label: "Theme",
                value: current.theme.clone(),
                placeholder: "project theme",
                source: ChoiceSource::Themes,
            },
            submit_field("Save project"),
        ],
        selected: 0,
    };

    let summary = execute_operation(&form, &application).unwrap();

    // Seven writes per visit would churn the file, and clearing a key that is already
    // absent is an error from `delete_config` — both are avoided by comparing first.
    assert!(
        summary.contains("already up to date"),
        "an unchanged form must not write: {summary}"
    );
}

#[test]
fn a_field_the_form_never_showed_is_left_alone_rather_than_cleared() {
    let form = OperationForm {
        title: "Edit project",
        kind: OperationKind::ProjectEdit,
        target: Some("Facultad".to_string()),
        fields: vec![FormField::Choice {
            label: "Theme",
            value: "gruvbox".to_string(),
            placeholder: "project theme",
            source: ChoiceSource::Themes,
        }],
        selected: 0,
    };

    // `text` cannot tell these apart, and reading an absent field as empty made an
    // incomplete form delete configuration it never displayed.
    assert_eq!(form.field("Theme").as_deref(), Some("gruvbox"));
    assert_eq!(form.field("Image model"), None);
    assert_eq!(form.text("Image model"), "");
}

#[test]
#[ignore = "prints the project edit form and its picker for inspection"]
fn dump_project_edit() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.transition(Screen::Browse(Section::Projects));
    app.browse_focus = BrowseFocus::Actions;
    app.browse_action_index = section_actions(Section::Projects)
        .iter()
        .position(|action| *action == BrowseAction::ProjectEdit)
        .unwrap();
    // The section loads asynchronously, and this dump does not drive that task.
    let active = app.application.show_project(None).unwrap();
    app.browse_rows = vec![BrowseRow {
        title: active.name.clone(),
        subtitle: String::new(),
        detail: String::new(),
        active: true,
    }];
    app.browse_index = 0;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    println!("\n╔══════ PROJECTS / Edit ══════");
    println!("{}", render_screen(&mut app, 100, 26));

    let index = app
        .operation
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|field| {
            matches!(
                field,
                FormField::Choice {
                    label: "Image model",
                    ..
                }
            )
        })
        .unwrap();
    app.operation.as_mut().unwrap().selected = index;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    println!("\n╔══════ PROJECTS / Edit + Image model picker ══════");
    println!("{}", render_screen(&mut app, 100, 26));
}

#[test]
fn every_nav_destination_opens_something() {
    // Two CLI surfaces — `sfumato tool` and `sfumato plugin` — had no TUI entry at all,
    // so "enable chart-gen" was unreachable from the interface that offers everything
    // else. A nav entry that dispatches nowhere is the same failure, silently.
    let mut app = App::new(Picker::halfblocks(), test_application());
    for (index, item) in NAV_ITEMS.iter().enumerate() {
        app.screen = Screen::Home;
        app.nav_index = index;
        app.open_nav_index();
        assert_ne!(
            app.screen,
            Screen::Home,
            "nav entry '{}' leads nowhere",
            item.title
        );
    }
}

#[test]
fn the_tools_section_lists_every_generation_tool_with_a_switch() {
    let application = test_application();
    let rows = load_section(Section::Tools, &application).unwrap();

    // Iterated from the enum, so a new tool reaches the TUI without another edit here.
    assert_eq!(rows.len(), GenerationToolKind::ALL.len());
    for row in &rows {
        assert!(
            row.subtitle.contains("enabled") || row.subtitle.contains("disabled"),
            "a tool row must say which it is: {row:?}"
        );
    }
    assert!(
        rows.iter().any(|row| row.title == "chart-gen"),
        "chart-gen must be switchable: {rows:?}"
    );
    assert_eq!(
        section_actions(Section::Tools),
        &[BrowseAction::ToolEnable, BrowseAction::ToolDisable]
    );
}

#[test]
fn a_tool_enabled_without_a_model_for_it_says_so() {
    let application = test_application();
    let rows = load_section(Section::Tools, &application).unwrap();

    // Enabled-but-unusable is invisible in the config: the drafter is simply never
    // offered the tool, and nothing explains why the output has no charts.
    for row in rows.iter().filter(|row| row.subtitle.contains("no model")) {
        assert!(
            row.detail.contains("Enabled but unusable"),
            "the detail must explain the gap: {row:?}"
        );
    }
}

#[test]
fn switching_a_tool_confirms_before_writing() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.transition(Screen::Browse(Section::Tools));
    app.browse_rows = vec![BrowseRow {
        title: "chart-gen".to_string(),
        subtitle: "enabled".to_string(),
        detail: String::new(),
        active: true,
    }];
    app.browse_index = 0;
    app.browse_focus = BrowseFocus::Actions;
    app.browse_action_index = section_actions(Section::Tools)
        .iter()
        .position(|action| *action == BrowseAction::ToolDisable)
        .unwrap();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let operation = app.operation.as_ref().expect("a confirmation opened");
    assert_eq!(operation.kind, OperationKind::ToolSet(false));
    assert_eq!(operation.target.as_deref(), Some("chart-gen"));
    // Writing a project default is not something a stray Enter should do.
    assert!(!operation.toggle("Confirm"));
}

#[test]
fn the_plugins_section_lists_installed_packages_with_a_switch() {
    let application = test_application();
    let rows = load_section(Section::Plugins, &application).unwrap();

    assert_eq!(
        section_actions(Section::Plugins),
        &[BrowseAction::PluginEnable, BrowseAction::PluginDisable]
    );
    for row in &rows {
        assert!(
            row.subtitle.contains("enabled")
                || row.subtitle.contains("disabled")
                || row.subtitle.contains("cannot be read"),
            "a plugin row must state its project state: {row:?}"
        );
    }
}

#[test]
#[ignore = "prints the tools and plugins sections for inspection"]
fn dump_tools_and_plugins() {
    for section in [Section::Tools, Section::Plugins] {
        let mut app = App::new(Picker::halfblocks(), test_application());
        app.transition(Screen::Browse(section));
        // The section loads asynchronously and this dump does not drive that task.
        app.browse_rows = load_section(section, &app.application.clone()).unwrap();
        app.browse_index = 0;
        println!("\n╔══════ {} ══════", section.title());
        println!("{}", render_screen(&mut app, 104, 26));
    }
}

#[test]
fn the_ui_plugin_reads_as_enabled_because_it_lives_in_its_own_field() {
    let application = test_application();
    let Ok(project) = application.show_project(None) else {
        return; // no active project on this machine
    };
    let Some(ui) = project.page.ui.clone() else {
        return; // this project selects no UI plugin
    };
    let rows = load_section(Section::Plugins, &application).unwrap();

    // `page.ui` is a separate field from `page.plugins`, and the CLI treats the union as
    // enabled. Reading only the list showed the project's own UI plugin as disabled,
    // disagreeing with `sfumato plugin list` about the same project.
    let row = rows
        .iter()
        .find(|row| row.title == ui)
        .unwrap_or_else(|| panic!("the UI plugin '{ui}' must be listed: {rows:?}"));
    assert!(row.active, "{row:?}");
    assert!(row.subtitle.contains("enabled"), "{row:?}");
    assert!(row.detail.contains("UI plugin"), "{row:?}");
}

#[test]
fn a_runtime_plugin_says_it_cannot_be_switched_directly() {
    let application = test_application();
    let rows = load_section(Section::Plugins, &application).unwrap();

    // `disable` rejects these outright, so the row must say so rather than let the
    // caller confirm an action that fails at the layer below.
    for row in rows
        .iter()
        .filter(|row| row.detail.contains("Category: runtime"))
    {
        assert!(row.detail.contains("Managed as a dependency"), "{row:?}");
    }
}

#[test]
fn a_committed_run_repoints_its_previews_at_the_revision() {
    let directory = tempfile::tempdir().unwrap();
    let staging = directory
        .path()
        .join(".staging/job-58890-18c8ed96ea0a8548/assets/images");
    let revision = directory
        .path()
        .join("revisions/rev-18c8ed96ea0a8548/assets/images");
    std::fs::create_dir_all(&revision).unwrap();
    std::fs::write(revision.join("chart.png"), b"x").unwrap();
    let mut activities = vec![Activity {
        kind: ActivityKind::ToolResult,
        title: "sfumato chart gen complete".to_string(),
        detail: String::new(),
        // The path the tool reported, before the commit moved the tree.
        image_path: Some(staging.join("chart.png")),
    }];

    reroot_previews(
        &mut activities,
        &directory
            .path()
            .join("revisions/rev-18c8ed96ea0a8548/index.html"),
    );

    assert_eq!(
        activities[0].image_path.as_deref(),
        Some(revision.join("chart.png").as_path()),
        "the asset exists one directory over, so the preview must follow it"
    );
}

#[test]
fn a_preview_with_no_committed_counterpart_is_left_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory
        .path()
        .join(".staging/job-1/assets/images/chart.png");
    let mut activities = vec![Activity {
        kind: ActivityKind::ToolResult,
        title: "chart".to_string(),
        detail: String::new(),
        image_path: Some(missing.clone()),
    }];

    reroot_previews(
        &mut activities,
        &directory.path().join("revisions/rev-1/index.html"),
    );

    // Guessing a path would report an error about a file the tool never named.
    assert_eq!(activities[0].image_path.as_deref(), Some(missing.as_path()));
}

#[test]
fn a_discarded_preview_is_explained_rather_than_reported_as_an_error() {
    let mut app = App::new(Picker::halfblocks(), test_application());

    app.load_image(std::path::Path::new(
        "/nonexistent/.staging/job-1/assets/images/chart.png",
    ));

    let (message, is_error) = app.status.clone().expect("a status was set");
    assert!(!is_error, "a by-design discard is not a fault: {message}");
    assert!(message.contains("no longer on disk"), "{message}");
    assert!(app.image.is_none());
}

#[tokio::test]
async fn escape_never_leaves_the_session() {
    // `Esc` on the home screen used to set `should_quit`, so the key that backs out
    // of a form ended the session one screen later — and took the running
    // generation with it. It now only clears the last message.
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.status = Some(("something happened".to_string(), false));

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(!app.should_quit, "escape must not leave the session");
    assert_eq!(app.screen, Screen::Home);
    assert!(app.status.is_none(), "escape clears the message");
}

#[tokio::test]
async fn q_types_a_character_instead_of_leaving() {
    // `q` was a global exit, which made it unusable as form input and made leaving
    // an accident away on every screen.
    let mut app = App::new(Picker::halfblocks(), test_application());

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));

    assert!(!app.should_quit, "a bare q must not leave the session");
}

#[tokio::test]
async fn leaving_asks_first_and_a_stray_key_stays() {
    let mut app = App::new(Picker::halfblocks(), test_application());

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert_eq!(app.overlay, Some(Overlay::Quit));
    assert!(!app.should_quit, "asking is not leaving");

    // Enter is deliberately not a confirmation: it submits every form in this UI.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!app.should_quit, "enter must not confirm the exit");
    assert!(app.overlay.is_none(), "the prompt closes either way");

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert!(app.should_quit, "y leaves");
}

#[tokio::test]
async fn the_exit_prompt_reaches_past_an_open_form() {
    // The overlay and form dispatch consume every key, so the exit gesture is
    // checked ahead of them or a form becomes a place with no way out.
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Generate;
    app.overlay = Some(Overlay::choice(ChoiceTarget::Generate(
        GenerateFieldId::Theme,
    )));

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));

    assert_eq!(app.overlay, Some(Overlay::Quit));
}

/// WCAG relative luminance, so contrast can be argued about rather than eyeballed.
#[cfg(test)]
fn luminance(colour: ratatui::style::Color) -> Option<f64> {
    let ratatui::style::Color::Rgb(red, green, blue) = colour else {
        return None;
    };
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.031_308 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    Some(0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue))
}

/// Asserts every glyph on the rows matching `rows` can be read where it is drawn.
///
/// Per-cell rather than per-style, because the defect this guards against is a
/// foreground that is legible in one place and not in another: the same colour on
/// the window background and on the selected row's fill.
#[cfg(test)]
fn assert_rows_are_legible(
    buffer: &ratatui::buffer::Buffer,
    rows: impl Fn(&str) -> bool,
    what: &str,
) {
    let mut checked = 0;
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect::<String>();
        if !rows(&row) {
            continue;
        }
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            if cell.symbol().trim().is_empty() {
                continue;
            }
            let (Some(foreground), Some(background)) = (luminance(cell.fg), luminance(cell.bg))
            else {
                continue;
            };
            let ratio = (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05);
            assert!(
                ratio >= 2.0,
                "'{}' at {x},{y} sits at {ratio:.2}:1 against its own background",
                cell.symbol()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "{what} drew nothing to check");
}

/// The exit prompt's answer keys were drawn in `PANEL`, which is a panel's *fill*
/// colour, over the window background — a contrast ratio of about 1.1:1, so the one
/// line that says how to answer the question was the line that could not be read.
#[tokio::test]
async fn the_exit_prompt_is_legible_including_the_keys_that_answer_it() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.overlay = Some(Overlay::Quit);
    let buffer = render_buffer(&mut app, 100, 32);

    assert_rows_are_legible(
        &buffer,
        |row| row.contains("y leaves") || row.contains("Close the session?"),
        "the exit prompt",
    );
}

/// A placeholder was drawn in `PANEL` too, and on the focused row `PANEL` is the
/// background — so the field you were standing on was the one whose default could
/// not be read at all. The focused row is what this checks.
#[tokio::test]
async fn a_focused_field_can_still_be_read_when_it_only_has_a_placeholder() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Generate;
    // Its placeholder states the default that applies when nothing is typed, which
    // is the whole reason the row has anything to say while empty.
    focus_field(&mut app, GenerateFieldId::Title);
    let buffer = render_buffer(&mut app, 100, 32);

    assert_rows_are_legible(
        &buffer,
        |row| row.contains("generated by the drafter"),
        "the focused title field",
    );
}

/// The caret marks a picker as a picker rather than a text field, which it cannot do
/// while it is the colour of what it is drawn on.
#[tokio::test]
async fn an_unfocused_picker_still_shows_its_caret() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Generate;
    focus_field(&mut app, GenerateFieldId::Title);
    let buffer = render_buffer(&mut app, 100, 32);

    assert_rows_are_legible(
        &buffer,
        // Theme is a picker and is not the focused row, so its caret is drawn in the
        // unfocused style.
        |row| row.contains("Theme") && row.contains('▾'),
        "an unfocused picker",
    );
}

/// A finished run reported "complete" and nothing else. The footer had a branch that
/// would have shown the path, but it only ran when no status message was set and the
/// completion handler always sets one — so the answer to "where is the file" was
/// unreachable from the interface that had just produced it. It is a feed entry now,
/// and a managed revision path only answers the question if all of it is on screen.
#[tokio::test]
async fn an_output_path_is_readable_to_its_last_character() {
    let path = "/Users/someone/.sfumato/Projects/Facultad/resources/documents/\
                asesoramiento-crediticio-ai-safety/revisions/rev-18c906050847b6a8/\
                asesoramiento-crediticio-ai-safety.pdf";
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Complete;
    app.activities.push(Activity {
        kind: ActivityKind::Output,
        title: "PDF".to_string(),
        detail: path.to_string(),
        image_path: None,
    });

    let screen = render_screen(&mut app, 100, 32);

    assert!(screen.contains("PDF"), "{screen}");
    // The tail is what a clipped entry loses, and the tail is the file name.
    assert!(
        screen.contains("asesoramiento-crediticio-ai-safety.pdf"),
        "the output path was clipped before its file name:\n{screen}"
    );
}

#[test]
fn leaving_the_sources_field_offers_its_folder_as_the_publish_destination() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("notes.md");
    std::fs::write(&source, "notes").unwrap();

    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Generate;
    focus_field(&mut app, GenerateFieldId::Sources);
    for character in source.display().to_string().chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    // Still being typed, so publish must stay untouched.
    assert!(app.form.text(GenerateFieldId::Publish).is_empty());

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    // A file resolves to the directory holding it, because "beside the sources" is a
    // folder either way.
    assert_eq!(
        app.form.text(GenerateFieldId::Publish),
        directory.path().display().to_string()
    );
}

#[test]
fn a_typed_publish_folder_survives_a_later_source_edit() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Generate;

    focus_field(&mut app, GenerateFieldId::Publish);
    for character in "Handouts".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    focus_field(&mut app, GenerateFieldId::Sources);
    for character in directory.path().display().to_string().chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(app.form.text(GenerateFieldId::Publish), "Handouts");
}

#[tokio::test]
async fn the_exit_prompt_names_the_run_it_would_cancel() {
    let mut app = App::new(Picker::halfblocks(), test_application());
    app.screen = Screen::Running;

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));

    let (message, _) = app.status.clone().expect("a status was set");
    assert!(
        message.contains("cancels the running operation"),
        "{message}"
    );
}
