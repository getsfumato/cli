use super::*;

#[test]
fn composes_generated_content_into_the_single_slot() {
    let template = GenerationTemplate {
        root: PathBuf::from("/templates/lecture"),
        manifest: GenerationTemplateManifest {
            schema_version: TEMPLATE_SCHEMA_VERSION,
            name: "lecture".into(),
            kind: TemplateKind::Slides,
            description: "Lecture structure".into(),
            source: PathBuf::from("template.md"),
        },
        source: format!("header\n{TEMPLATE_CONTENT_SLOT}\nfooter"),
    };

    assert_eq!(
        template.compose("  lesson  ").unwrap(),
        "header\nlesson\nfooter"
    );
}

#[test]
fn validates_portable_names_and_exactly_one_slot() {
    assert!(validate_template_name("fourier-lecture").is_ok());
    assert!(validate_template_name("../lecture").is_err());
    assert!(validate_template_source(TEMPLATE_CONTENT_SLOT).is_ok());
    assert!(validate_template_source("missing").is_err());
    assert!(
        validate_template_source(&format!("{TEMPLATE_CONTENT_SLOT}{TEMPLATE_CONTENT_SLOT}"))
            .is_err()
    );
}
