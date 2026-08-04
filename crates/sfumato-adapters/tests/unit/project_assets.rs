use super::*;

#[test]
fn adds_lists_loads_and_removes_project_images() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let source = temporary.path().join("university-logo.png");
    std::fs::write(&source, b"not-a-decoder-test").unwrap();
    let catalog = FilesystemProjectAssetCatalog;

    let added = catalog
        .add(
            &project,
            AddProjectAssetRequest {
                source: &source,
                name: Some("logo"),
                theme: "gruvbox",
                metadata: ProjectAssetMetadata {
                    description: "University logo used on title pages".into(),
                    alt_text: "University logo".into(),
                    tags: vec!["branding".into()],
                    generation_prompt: None,
                },
            },
        )
        .unwrap();
    assert_eq!(added.name, "logo");
    assert_eq!(catalog.list(&project).unwrap().entries.len(), 1);
    assert_eq!(
        catalog.load(&project, "logo").unwrap().variants["gruvbox"].content_hash,
        added.variants["gruvbox"].content_hash
    );

    let removed = catalog.remove(&project, "logo").unwrap();
    assert!(!removed.variants["gruvbox"].path.exists());
    assert!(catalog.list(&project).unwrap().entries.is_empty());
}

#[test]
fn rejects_unsafe_svg_assets() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("bad.svg");
    std::fs::write(&source, r#"<svg onload="alert(1)"></svg>"#).unwrap();
    let catalog = FilesystemProjectAssetCatalog;

    let error = catalog
        .add(
            temporary.path(),
            AddProjectAssetRequest {
                source: &source,
                name: Some("bad"),
                theme: "*",
                metadata: ProjectAssetMetadata {
                    description: "Unsafe".into(),
                    ..Default::default()
                },
            },
        )
        .unwrap_err();

    assert!(error.message.contains("forbidden"));
}

#[test]
fn resolves_exact_theme_before_wildcard_and_preserves_metadata() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("diagram.png");
    std::fs::write(&source, b"theme-independent").unwrap();
    let catalog = FilesystemProjectAssetCatalog;
    catalog
        .add(
            temporary.path(),
            AddProjectAssetRequest {
                source: &source,
                name: Some("diagram"),
                theme: "*",
                metadata: ProjectAssetMetadata {
                    description: "A concept diagram".into(),
                    alt_text: "Concept relationships".into(),
                    tags: vec!["concept".into()],
                    generation_prompt: Some("Draw the same concept relationships".into()),
                },
            },
        )
        .unwrap();
    catalog
        .add_generated_variant(
            temporary.path(),
            "diagram",
            "ferrari",
            "image/png",
            b"ferrari-version",
        )
        .unwrap();

    let asset = catalog.load(temporary.path(), "diagram").unwrap();
    assert_eq!(asset.resolve("ferrari").unwrap().theme, "ferrari");
    assert_eq!(asset.resolve("gruvbox").unwrap().theme, "*");
    assert_eq!(asset.metadata.tags, vec!["concept"]);
}

#[test]
fn migrates_schema_one_assets_to_wildcard_variants() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join(".sfumato/assets");
    std::fs::create_dir_all(root.join("files")).unwrap();
    std::fs::write(root.join("files/old.png"), b"legacy").unwrap();
    let hash = format!("{:x}", Sha256::digest(b"legacy"));
    std::fs::write(
        root.join("manifest.toml"),
        format!(
            r#"schema_version = 1

[assets.old]
description = "Legacy diagram"
media_type = "image/png"
filename = "old.png"
file = "files/old.png"
content_hash = "{hash}"
"#
        ),
    )
    .unwrap();

    let asset = FilesystemProjectAssetCatalog
        .load(temporary.path(), "old")
        .unwrap();

    assert_eq!(asset.resolve("ferrari").unwrap().theme, "*");
    assert!(
        std::fs::read_to_string(root.join("manifest.toml"))
            .unwrap()
            .contains("schema_version = 2")
    );
}

#[test]
fn an_inferred_asset_name_transliterates_instead_of_mangling() {
    // The local slug lowercased ASCII only and turned everything else into `-`, so
    // an accented filename produced a name that reads as a typo — and that name is
    // the asset's identity for `artifact show`, `artifact remove`, and references
    // from generated resources.
    for (name, expected) in [
        ("Café", "cafe"),
        ("Diseño", "diseno"),
        ("Añejo", "anejo"),
        ("Logotipo Ñandú", "logotipo-nandu"),
        ("Ícono Público", "icono-publico"),
    ] {
        assert_eq!(slug(name), expected, "{name}");
    }
}

#[test]
fn two_different_names_no_longer_collide_on_one_slug() {
    // `Diseño` and `Dise o` both produced `dise-o`, so one asset could silently
    // take another's identity.
    assert_ne!(slug("Diseño"), slug("Dise o"));
}

#[test]
fn plain_ascii_names_are_unchanged() {
    assert_eq!(slug("Fourier Series"), "fourier-series");
    assert_eq!(slug("logo"), "logo");
}
