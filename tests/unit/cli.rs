use super::*;

#[test]
fn parses_reviewer_model_and_review_opt_out() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "slides",
        "--instruction",
        "Explain Fourier series",
        "--review-model",
        "local-review",
        "--no-review",
    ])
    .unwrap();

    let Some(Commands::Generate {
        command: GenerateCommands::Slides(args),
    }) = cli.command
    else {
        panic!("expected generate slides command");
    };
    assert_eq!(args.review_model.as_deref(), Some("local-review"));
    assert!(args.no_review);
}

#[test]
fn model_use_accepts_reviewer_role() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "model",
        "use",
        "reviewer",
        "local-review",
        "--project",
        "university",
    ])
    .unwrap();

    let Some(Commands::Model {
        command: ModelCommands::Use(args),
    }) = cli.command
    else {
        panic!("expected model use command");
    };
    assert_eq!(args.selector, "reviewer");
    assert_eq!(args.profile, "local-review");
    assert_eq!(args.project.as_deref(), Some("university"));
}

#[test]
fn parses_focused_slide_edit_command() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "edit",
        "slides",
        "deck.md",
        "--instruction",
        "Clarify the definition on slide two",
        "--project",
        "university",
        "--model",
        "text=cloud-draft",
    ])
    .unwrap();

    let Some(Commands::Edit {
        command: EditCommands::Slides(args),
    }) = cli.command
    else {
        panic!("expected edit slides command");
    };
    assert_eq!(args.markdown_path, PathBuf::from("deck.md"));
    assert_eq!(args.project.as_deref(), Some("university"));
    assert_eq!(args.model_overrides, vec!["text=cloud-draft"]);
}

#[test]
fn parses_page_plugins_and_dedicated_generation_options() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "page",
        "notes",
        "--instruction",
        "Explain Fourier series interactively",
        "--plugin",
        "threejs",
        "--plugin",
        "motion",
        "--theme",
        "gruvbox",
        "--model",
        "text=cloud-draft",
    ])
    .unwrap();

    let Some(Commands::Generate {
        command: GenerateCommands::Page(args),
    }) = cli.command
    else {
        panic!("expected generate page command");
    };
    assert_eq!(args.inputs, vec![PathBuf::from("notes")]);
    assert_eq!(args.plugins, vec!["threejs", "motion"]);
    assert_eq!(args.theme.as_deref(), Some("gruvbox"));
    assert_eq!(args.model_overrides, vec!["text=cloud-draft"]);
}

#[test]
fn parses_page_plugin_discovery() {
    let cli = Cli::try_parse_from(["sfumato", "plugin", "show", "threejs"]).unwrap();
    let Some(Commands::Plugin {
        command: PluginCommands::Show(args),
    }) = cli.command
    else {
        panic!("expected plugin show command");
    };
    assert_eq!(args.id, "threejs");
}
