use super::*;

#[test]
fn creates_and_round_trips_a_manifest() {
    let mut manifest = ArtifactManifest::new(
        ArtifactId::new("deck-final").unwrap(),
        JobId::new("job-01").unwrap(),
        RevisionId::new("abc123").unwrap(),
        ArtifactKind::Pdf,
    )
    .with_media_type("application/pdf")
    .unwrap();
    manifest.metadata.insert("pages", "12").unwrap();

    let encoded = serde_json::to_string(&manifest).unwrap();
    let decoded: ArtifactManifest = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, manifest);
    assert_eq!(decoded.metadata.get("pages"), Some("12"));
    decoded.validate().unwrap();
}

#[test]
fn rejects_invalid_free_form_metadata() {
    let mut metadata = ArtifactMetadata::new();
    assert!(metadata.insert("bad key", "value").is_err());
    assert!(metadata.insert("title", "bad\nvalue").is_err());
}
