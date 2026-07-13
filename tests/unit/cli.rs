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
