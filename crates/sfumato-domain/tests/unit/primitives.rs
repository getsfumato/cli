use super::*;

#[test]
fn validates_portable_ids_during_construction_and_deserialization() {
    assert_eq!(JobId::new("job-01").unwrap().as_str(), "job-01");
    assert!(ArtifactId::new("../artifact").is_err());
    assert!(serde_json::from_str::<RevisionId>(r#""bad revision""#).is_err());
}

#[test]
fn preserves_existing_name_rules() {
    assert!(ProjectName::new("Fourier course").is_ok());
    assert!(ProjectName::new("../course").is_err());
    assert!(ModelProfileName::new("local-text").is_ok());
    assert!(ModelProfileName::new("Local Text").is_err());
    assert!(ThemeName::new("sfumato-default").is_ok());
}

#[test]
fn parses_capabilities_case_insensitively() {
    assert_eq!("IMAGE".parse::<Capability>().unwrap(), Capability::Image);
    assert!("music".parse::<Capability>().is_err());
}

#[test]
fn secret_references_are_indirect_and_structured() {
    let reference = SecretRef::environment("OPENAI_API_KEY").unwrap();

    assert_eq!(reference.as_str(), "env:OPENAI_API_KEY");
    assert_eq!(reference.scheme(), "env");
    assert_eq!(reference.target(), "OPENAI_API_KEY");
    assert!(SecretRef::try_from("raw-secret").is_err());
    assert!(SecretRef::environment("lowercase").is_err());
}
