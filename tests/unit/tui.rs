use super::*;

fn test_application() -> Arc<SfumatoApplication> {
    Arc::new(sfumato_adapters::application::production_application().unwrap())
}

#[test]
fn generation_form_requires_an_instruction() {
    let form = GenerateForm::default();

    assert!(form.to_args().is_err());
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

    let args = form.to_args().unwrap();

    assert_eq!(
        args.inputs,
        vec![PathBuf::from("notes"), PathBuf::from("examples")]
    );
    assert_eq!(args.model_overrides, vec!["text=cloud-draft"]);
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
