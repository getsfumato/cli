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
fn parses_secure_connector_authentication_commands() {
    for command_name in ["login", "auth-status", "logout"] {
        let cli =
            Cli::try_parse_from(["sfumato", "connector", command_name, "openrouter"]).unwrap();
        let Some(Commands::Connector { command }) = cli.command else {
            panic!("expected connector command");
        };
        let name = match command {
            ConnectorCommands::Login(args)
            | ConnectorCommands::AuthStatus(args)
            | ConnectorCommands::Logout(args) => args.name,
            _ => panic!("expected connector authentication command"),
        };
        assert_eq!(name, "openrouter");
    }
}

#[test]
fn connector_setup_defaults_to_stored_credentials_and_accepts_explicit_env() {
    let stored = Cli::try_parse_from(["sfumato", "connector", "setup", "openrouter"]).unwrap();
    let Some(Commands::Connector {
        command: ConnectorCommands::Setup(stored),
    }) = stored.command
    else {
        panic!("expected connector setup command");
    };
    assert!(stored.api_key_env.is_none());

    let environment = Cli::try_parse_from([
        "sfumato",
        "connector",
        "setup",
        "openrouter",
        "--api-key-env",
        "CI_OPENROUTER_KEY",
    ])
    .unwrap();
    let Some(Commands::Connector {
        command: ConnectorCommands::Setup(environment),
    }) = environment.command
    else {
        panic!("expected connector setup command");
    };
    assert_eq!(
        environment.api_key_env.as_deref(),
        Some("CI_OPENROUTER_KEY")
    );
}

#[test]
fn parses_local_codex_connector_setup() {
    let cli = Cli::try_parse_from(["sfumato", "connector", "setup", "codex"]).unwrap();
    let Some(Commands::Connector {
        command: ConnectorCommands::Setup(args),
    }) = cli.command
    else {
        panic!("expected connector setup command");
    };

    assert!(matches!(args.preset, ConnectorPreset::Codex));
    assert!(args.api_key_env.is_none());
}

#[test]
fn parses_connector_native_model_discovery() {
    let cli = Cli::try_parse_from(["sfumato", "connector", "models", "codex"]).unwrap();
    let Some(Commands::Connector {
        command: ConnectorCommands::Models(args),
    }) = cli.command
    else {
        panic!("expected connector models command");
    };
    assert_eq!(args.name, "codex");
}

#[test]
fn parses_connector_capabilities_and_status() {
    for command_name in ["capabilities", "status"] {
        let cli =
            Cli::try_parse_from(["sfumato", "connector", command_name, "openrouter"]).unwrap();
        let Some(Commands::Connector { command }) = cli.command else {
            panic!("expected connector command");
        };
        let name = match command {
            ConnectorCommands::Capabilities(args) | ConnectorCommands::Status(args) => args.name,
            _ => panic!("expected connector introspection command"),
        };
        assert_eq!(name, "openrouter");
    }
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
fn generation_does_not_select_a_template_implicitly() {
    let page = Cli::try_parse_from([
        "sfumato",
        "generate",
        "page",
        "--instruction",
        "Explain Fourier series",
    ])
    .unwrap();
    let Some(Commands::Generate {
        command: GenerateCommands::Page(page),
    }) = page.command
    else {
        panic!("expected page generator");
    };
    assert!(page.template.is_none());

    let slides = Cli::try_parse_from([
        "sfumato",
        "generate",
        "slides",
        "--instruction",
        "Explain Fourier series",
    ])
    .unwrap();
    let Some(Commands::Generate {
        command: GenerateCommands::Slides(slides),
    }) = slides.command
    else {
        panic!("expected slides generator");
    };
    assert!(slides.template.is_none());
}

#[test]
fn parses_ui_as_an_exclusive_page_library() {
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
    assert_eq!(args.ui.as_deref(), Some("materialui"));
    assert!(args.plugins.is_empty());
}

#[test]
fn parses_video_generation_and_engine_specific_options() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "video",
        "notes",
        "--instruction",
        "Explain Fourier series",
        "--engine",
        "manim",
        "--duration",
        "20",
        "--fps",
        "60",
        "--quality",
        "high",
        "--allow-code-execution",
        "--model",
        "code=codex",
    ])
    .unwrap();
    let Some(Commands::Generate {
        command: GenerateCommands::Video(args),
    }) = cli.command
    else {
        panic!("expected generate video command");
    };

    assert!(matches!(args.engine, VideoEngineArg::Manim));
    assert_eq!(args.duration, 20);
    assert_eq!(args.fps, Some(60));
    assert_eq!(args.quality.as_deref(), Some("high"));
    assert!(args.allow_code_execution);
    assert_eq!(args.model_overrides, vec!["code=codex"]);
}

#[test]
fn parses_generation_tool_and_renderer_management() {
    let tool = Cli::try_parse_from([
        "sfumato",
        "tool",
        "enable",
        "video-gen",
        "--project",
        "university",
    ])
    .unwrap();
    let Some(Commands::Tool {
        command: ToolCommands::Enable(args),
    }) = tool.command
    else {
        panic!("expected tool enable command");
    };
    assert!(matches!(args.tool, GenerationToolArg::VideoGen));

    let renderer = Cli::try_parse_from(["sfumato", "renderer", "doctor", "hyperframe"]).unwrap();
    let Some(Commands::Renderer {
        command: RendererCommands::Doctor(args),
    }) = renderer.command
    else {
        panic!("expected renderer doctor command");
    };
    assert!(matches!(
        args.renderer,
        Some(LocalVideoRendererArg::Hyperframe)
    ));
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
        "artifact",
        "edit",
        "square-wave-spectrum",
        "--project",
        "university",
        "--alt-text",
        "Odd harmonic spectrum",
        "--tag",
        "fourier",
        "--prompt",
        "Recreate the same diagram for the selected theme",
        "--from-theme",
        "*",
        "--to-theme",
        "gruvbox",
    ])
    .unwrap();
    let Some(Commands::Artifact {
        command: ArtifactCommands::Edit(args),
    }) = cli.command
    else {
        panic!("expected artifact edit command");
    };
    assert_eq!(args.name, "square-wave-spectrum");
    assert_eq!(args.tags, vec!["fourier"]);
    assert_eq!(args.from_theme.as_deref(), Some("*"));
    assert_eq!(args.to_theme.as_deref(), Some("gruvbox"));

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
