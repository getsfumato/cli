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
        "--shadcn",
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
    assert!(args.shadcn);
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

#[test]
fn parses_plugin_install_update_and_project_enablement() {
    let install = Cli::try_parse_from([
        "sfumato",
        "plugin",
        "install",
        "shadcn",
        "--version",
        "0.1.0",
    ])
    .unwrap();
    let Some(Commands::Plugin {
        command: PluginCommands::Install(args),
    }) = install.command
    else {
        panic!("expected plugin install command");
    };
    assert_eq!(args.id, "shadcn");
    assert_eq!(args.version.as_deref(), Some("0.1.0"));

    let enable = Cli::try_parse_from([
        "sfumato",
        "plugin",
        "enable",
        "shadcn",
        "--project",
        "university",
    ])
    .unwrap();
    let Some(Commands::Plugin {
        command: PluginCommands::Enable(args),
    }) = enable.command
    else {
        panic!("expected plugin enable command");
    };
    assert_eq!(args.project.as_deref(), Some("university"));
}

#[test]
fn pages_alias_selects_the_page_generator() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "pages",
        "--instruction",
        "Explain Fourier series",
        "--shadcn",
    ])
    .unwrap();
    let Some(Commands::Generate {
        command: GenerateCommands::Page(args),
    }) = cli.command
    else {
        panic!("expected page generator");
    };
    assert!(args.shadcn);
}

#[test]
fn parses_reusable_template_commands_and_generation_selection() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "template",
        "create",
        "lecture",
        "--kind",
        "slides",
        "--from",
        "lecture.md",
    ])
    .unwrap();
    let Some(Commands::Template {
        command: TemplateCommands::Create(args),
    }) = cli.command
    else {
        panic!("expected template create command");
    };
    assert_eq!(args.name, "lecture");
    assert!(matches!(args.kind, TemplateKindArg::Slides));
    assert_eq!(args.source, Some(PathBuf::from("lecture.md")));

    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "page",
        "--instruction",
        "Explain Fourier series",
        "--template",
        "interactive-lesson",
    ])
    .unwrap();
    let Some(Commands::Generate {
        command: GenerateCommands::Page(args),
    }) = cli.command
    else {
        panic!("expected generate page command");
    };
    assert_eq!(args.template.as_deref(), Some("interactive-lesson"));
}

#[test]
fn accepts_ui_as_a_generic_page_plugin_alias() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "page",
        "--instruction",
        "Build an interactive lesson",
        "--ui",
        "materialui",
    ])
    .unwrap();
    let Some(Commands::Generate {
        command: GenerateCommands::Page(args),
    }) = cli.command
    else {
        panic!("expected generate page command");
    };
    assert_eq!(args.plugins, vec!["materialui"]);
}

#[test]
fn parses_project_artifacts_and_design_md_exchange() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "artifact",
        "add",
        "logo.png",
        "--name",
        "university-logo",
        "--project",
        "university",
    ])
    .unwrap();
    let Some(Commands::Artifact {
        command: ArtifactCommands::Add(args),
    }) = cli.command
    else {
        panic!("expected artifact add command");
    };
    assert_eq!(args.name.as_deref(), Some("university-logo"));
    assert_eq!(args.project.as_deref(), Some("university"));

    let cli = Cli::try_parse_from([
        "sfumato",
        "theme",
        "import",
        "DESIGN.md",
        "--name",
        "gruvbox",
    ])
    .unwrap();
    let Some(Commands::Theme {
        command: ThemeCommands::Import(args),
    }) = cli.command
    else {
        panic!("expected theme import command");
    };
    assert_eq!(args.path, PathBuf::from("DESIGN.md"));
    assert_eq!(args.name.as_deref(), Some("gruvbox"));
}
