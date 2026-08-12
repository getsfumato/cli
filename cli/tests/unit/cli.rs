use super::*;

/// Unwraps a parsed `generate`, past the box the variant is held in.
///
/// `Commands::Generate` carries every generate flag there is and would
/// otherwise set the size of the whole enum, so it is boxed — and a `Box` is not
/// something a pattern can see through on stable.
fn generate(cli: Cli) -> GenerateCommands {
    match cli.command {
        Some(Commands::Generate { command }) => *command,
        other => panic!("expected a generate command, got: {other:?}"),
    }
}

#[test]
fn parses_video_approval_output_override() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "video",
        "approve",
        "lesson-review",
        "--project",
        "university",
        "--out",
        "published-course",
    ])
    .unwrap();

    let Some(Commands::Video {
        command: VideoCommands::Approve(args),
    }) = cli.command
    else {
        panic!("expected video approve command");
    };
    assert_eq!(args.review_id, "lesson-review");
    assert_eq!(args.project.as_deref(), Some("university"));
    assert_eq!(args.out, Some(PathBuf::from("published-course")));
}

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

    let GenerateCommands::Slides(args) = generate(cli) else {
        panic!("expected generate slides command");
    };
    assert_eq!(args.review_model.as_deref(), Some("local-review"));
    assert!(args.no_review);
}

#[test]
fn generate_takes_a_brain_for_one_run() {
    // `--project` is the Sfumato project and `--brain-project` the Vitruvio one.
    // Two registries, two names, and one command routinely states both.
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "slides",
        "--instruction",
        "explain Jacobi",
        "--project",
        "university",
        "--brain-project",
        "facultad",
        "--brain",
        "analisis-numerico",
    ])
    .unwrap();

    let GenerateCommands::Slides(args) = generate(cli) else {
        panic!("expected generate slides command");
    };
    assert_eq!(args.project.as_deref(), Some("university"));
    assert_eq!(args.brain_project.as_deref(), Some("facultad"));
    assert_eq!(args.brain.as_deref(), Some("analisis-numerico"));
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

    let GenerateCommands::Page(args) = generate(cli) else {
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
fn parses_connector_preset_listing() {
    let cli = Cli::try_parse_from(["sfumato", "connector", "presets"]).unwrap();

    assert!(matches!(
        cli.command,
        Some(Commands::Connector {
            command: ConnectorCommands::Presets,
        })
    ));
}

#[test]
fn cli_and_core_connector_presets_stay_in_sync() {
    // `tests/architecture.rs` forbids clap in `sfumato-core`, so the preset list
    // is mirrored in `cli.rs`. This test is what keeps the mirror honest: adding
    // a core preset without a clap variant (or vice versa) fails here.
    let cli_presets = <ConnectorPreset as clap::ValueEnum>::value_variants()
        .iter()
        .map(|preset| {
            sfumato_core::connectors::ConnectorPreset::from(*preset)
                .as_str()
                .to_string()
        })
        .collect::<std::collections::BTreeSet<_>>();
    let core_presets = sfumato_core::connectors::ConnectorPreset::ALL
        .into_iter()
        .map(|preset| preset.as_str().to_string())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(cli_presets, core_presets);
}

#[test]
fn every_connector_preset_parses_from_the_command_line() {
    for preset in sfumato_core::connectors::ConnectorPreset::ALL {
        let cli = Cli::try_parse_from(["sfumato", "connector", "setup", preset.as_str()])
            .unwrap_or_else(|error| panic!("preset '{preset}' should parse: {error}"));
        let Some(Commands::Connector {
            command: ConnectorCommands::Setup(args),
        }) = cli.command
        else {
            panic!("expected connector setup command for preset '{preset}'");
        };

        assert_eq!(
            sfumato_core::connectors::ConnectorPreset::from(args.preset),
            preset
        );
    }
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
    let GenerateCommands::Page(args) = generate(cli) else {
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
    let GenerateCommands::Page(args) = generate(cli) else {
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
    let GenerateCommands::Page(page) = generate(page) else {
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
    let GenerateCommands::Slides(slides) = generate(slides) else {
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
    let GenerateCommands::Page(args) = generate(cli) else {
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
    let GenerateCommands::Video(args) = generate(cli) else {
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

#[test]
fn parses_document_page_setup_flags() {
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "doc",
        "notes.md",
        "--instruction",
        "Study notes on message queues",
        "--page-size",
        "letter",
        "--no-toc",
        "--cover",
        "--out",
        "handouts",
    ])
    .unwrap();

    let GenerateCommands::Document(args) = generate(cli) else {
        panic!("expected generate document command");
    };
    assert_eq!(args.inputs, vec![PathBuf::from("notes.md")]);
    assert!(matches!(args.page_size, Some(DocumentPageSizeArg::Letter)));
    assert!(args.no_toc);
    assert!(!args.toc);
    assert!(args.cover);
    assert!(!args.no_cover);
    assert_eq!(args.out, Some(PathBuf::from("handouts")));
}

#[test]
fn omitting_document_page_setup_flags_defers_to_the_theme() {
    // An omitted flag is not the same as its negative form: the theme decides,
    // so the parsed args must be able to express "unspecified".
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "document",
        "--instruction",
        "Study notes",
    ])
    .unwrap();

    let GenerateCommands::Document(args) = generate(cli) else {
        panic!("expected generate document command");
    };
    assert!(args.page_size.is_none());
    assert!(!args.toc && !args.no_toc);
    assert!(!args.cover && !args.no_cover);
}

#[test]
fn the_last_document_page_setup_flag_wins() {
    // `--toc --no-toc` has to resolve to one answer rather than set both.
    let cli = Cli::try_parse_from([
        "sfumato",
        "generate",
        "docs",
        "--instruction",
        "Study notes",
        "--toc",
        "--no-toc",
    ])
    .unwrap();

    let GenerateCommands::Document(args) = generate(cli) else {
        panic!("expected generate document command");
    };
    assert!(args.no_toc);
    assert!(!args.toc);
}

/// Renders the command surface as Markdown, from the parser rather than by hand.
///
/// A reference that disagrees with the parser is worse than none: it costs a
/// turn to discover, and the reader has no way to tell which of the two is
/// lying. Generating it and checking the committed copy in CI means adding a
/// command forces the reference to be regenerated in the same change.
mod reference {
    use super::*;
    use clap::CommandFactory;
    use std::fmt::Write as _;

    /// Where the generated reference is committed.
    fn reference_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the cli package sits under the workspace root")
            .join("skills/sfumato/references/cli-reference.md")
    }

    /// One command's arguments and options, in the order clap declares them.
    fn signature(command: &clap::Command) -> String {
        let mut parts = Vec::new();
        for argument in command.get_positionals() {
            let name = argument.get_id().as_str().to_ascii_uppercase();
            parts.push(if argument.is_required_set() {
                format!("`<{name}>`")
            } else {
                format!("`[{name}]`")
            });
        }
        let mut flags: Vec<_> = command
            .get_arguments()
            .filter(|argument| argument.get_long().is_some())
            .filter(|argument| {
                // `--timeout` is global and would repeat on all seventy-odd
                // rows, and `--help` is not a command's own surface.
                !matches!(argument.get_long(), Some("help" | "timeout"))
            })
            .map(|argument| {
                let long = argument.get_long().unwrap_or_default();
                let note = if argument.is_required_set() {
                    " *(required)*"
                } else if argument.is_hide_set() {
                    // Accepted but absent from `--help`, which is exactly the
                    // kind of flag a reader cannot discover any other way.
                    " *(hidden)*"
                } else {
                    ""
                };
                format!("`--{long}`{note}")
            })
            .collect();
        parts.append(&mut flags);
        parts.join(" ")
    }

    /// Accepted values for whichever arguments constrain them.
    fn values(command: &clap::Command) -> Vec<String> {
        command
            .get_arguments()
            .filter(|argument| !matches!(argument.get_long(), Some("help" | "timeout")))
            .filter_map(|argument| {
                let accepted: Vec<_> = argument
                    .get_possible_values()
                    .iter()
                    .map(|value| value.get_name().to_string())
                    .collect();
                // A switch reports `true, false`, which is what a switch is and
                // tells a reader nothing they did not already know.
                if accepted.is_empty() || accepted == ["true", "false"] {
                    return None;
                }
                let name = argument.get_long().map_or_else(
                    || argument.get_id().as_str().to_ascii_uppercase(),
                    |long| format!("--{long}"),
                );
                Some(format!("`{name}`: {}", accepted.join(", ")))
            })
            .collect()
    }

    fn describe(command: &clap::Command) -> String {
        command
            .get_about()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    fn render(command: &clap::Command, path: &str, depth: usize, out: &mut String) {
        let heading = "#".repeat(depth.min(6));
        let signature = signature(command);
        let title = if signature.is_empty() {
            format!("`{path}`")
        } else {
            format!("`{path}` {signature}")
        };
        let _ = writeln!(out, "\n{heading} {title}\n");
        let about = describe(command);
        if !about.is_empty() {
            let _ = writeln!(out, "{about}\n");
        }
        for line in values(command) {
            let _ = writeln!(out, "- {line}");
        }
        let mut children: Vec<_> = command.get_subcommands().collect();
        children.sort_by_key(|child| child.get_name().to_string());
        for child in children {
            if child.get_name() == "help" {
                continue;
            }
            render(
                child,
                &format!("{path} {}", child.get_name()),
                depth + 1,
                out,
            );
        }
    }

    fn generate() -> String {
        let root = Cli::command();
        let mut out = String::from(
            "# CLI reference\n\n\
             Generated from the clap command declarations by the `renders_the_committed_cli_reference`\n\
             test in `cli/tests/unit/cli.rs`. Do not edit by hand: a reference that disagrees with the\n\
             parser is worse than none, because it costs a turn to discover which one is lying.\n\n\
             Regenerate with `SFUMATO_WRITE_CLI_REFERENCE=1 cargo test -p sfumato cli_reference`.\n\n\
             Every command also accepts the global `--timeout <SECONDS>`, which abandons the operation\n\
             after that many seconds and is unbounded when omitted. `--help` is omitted throughout.\n",
        );
        let mut groups: Vec<_> = root.get_subcommands().collect();
        groups.sort_by_key(|group| group.get_name().to_string());
        for group in groups {
            if group.get_name() == "help" {
                continue;
            }
            render(group, &format!("sfumato {}", group.get_name()), 2, &mut out);
        }
        out
    }

    /// Every command group is named in the hand-written surface skill.
    ///
    /// The generated reference cannot go stale, but the prose beside it can:
    /// `sfumato-cli/SKILL.md` names the flag that decides each command's
    /// outcome, which is a judgement no generator makes. A group added without
    /// a section there would simply be invisible to a reader who trusted it,
    /// and the pinned count in its opening line would quietly become wrong.
    #[test]
    fn the_command_surface_skill_documents_every_group() {
        let skill = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the cli package sits under the workspace root")
            .join("skills/sfumato-cli/SKILL.md");
        let prose = std::fs::read_to_string(&skill).expect("the surface skill is committed");

        let groups: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|group| group.get_name().to_string())
            .filter(|name| name != "help")
            .collect();

        for group in &groups {
            assert!(
                prose.contains(&format!("## `{group}`")),
                "skills/sfumato-cli/SKILL.md has no `## `{group}`` section. \
                 Document the group, or the reader who trusts that file cannot find it."
            );
        }

        // Spelled out in the prose, so it has to be corrected rather than left
        // to rot the first time a group is added.
        let counted = match groups.len() {
            15 => "Fifteen groups",
            other => panic!("the group count changed to {other}; update the count in the skill"),
        };
        assert!(
            prose.contains(counted),
            "the skill's opening line no longer states the real group count"
        );
    }

    #[test]
    fn renders_the_committed_cli_reference() {
        let generated = generate();
        let path = reference_path();
        if std::env::var_os("SFUMATO_WRITE_CLI_REFERENCE").is_some() {
            std::fs::write(&path, &generated).expect("the reference is writable");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            committed, generated,
            "skills/sfumato/references/cli-reference.md is stale. \
             Regenerate it with SFUMATO_WRITE_CLI_REFERENCE=1 cargo test -p sfumato cli_reference"
        );
    }
}
