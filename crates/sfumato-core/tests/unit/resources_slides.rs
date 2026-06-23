use super::*;
use crate::config::GlobalConfig;

fn effective_config() -> EffectiveConfig {
    let global = GlobalConfig::default_config();
    EffectiveConfig {
        user: global.user,
        project_name: "demo".to_string(),
        project_root: PathBuf::from("/tmp/demo"),
        output_dir: PathBuf::from("Resources/Sfumato"),
        theme: "sfumato-default".to_string(),
        connectors: global.connectors,
        models: global.models,
        model_defaults: global.defaults.0,
        marp: global.marp,
    }
}

#[test]
fn filters_supported_files() {
    assert!(is_supported(Path::new("note.md")));
    assert!(!is_supported(Path::new("image.png")));
}

#[test]
fn supports_no_source_files() {
    assert!(collect_sources(&[]).unwrap().is_empty());
}

#[test]
fn strips_markdown_code_fence() {
    let text = "```markdown\n---\nmarp: true\n---\n# Title\n```";
    assert!(strip_code_fence(text).starts_with("---"));
}

#[test]
fn rejects_paths_outside_output_root() {
    assert!(ensure_inside(Path::new("/tmp/out"), Path::new("/tmp/elsewhere/a.md")).is_err());
}

#[test]
fn normalizes_frontmatter() {
    let mut config = effective_config();
    config.theme = "gruvbox".to_string();
    let markdown = normalize_marp_markdown("# Demo\n\n---\n\n## One", &config, "Demo").unwrap();
    assert!(markdown.contains("marp: true"));
    assert!(markdown.contains("theme: gruvbox"));
}

#[test]
fn copies_theme_css_to_output() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("theme.css");
    let destination = temp.path().join("output/themes/demo.css");
    fs::write(&source, "/* @theme demo */").unwrap();
    let package = ThemePackage {
        root: temp.path().to_path_buf(),
        manifest: crate::themes::ThemeManifest {
            schema_version: crate::themes::THEME_SCHEMA_VERSION,
            name: "demo".to_string(),
            description: "Demo".to_string(),
            tokens: Default::default(),
            adapters: crate::themes::ThemeAdapters {
                marp_css: PathBuf::from("theme.css"),
                html: None,
            },
        },
    };
    copy_theme_css(&package, &destination).unwrap();
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        "/* @theme demo */"
    );
}

#[test]
fn summarizes_declared_tools_for_generation_output() {
    let tool = ToolDefinition {
        kind: "function".to_string(),
        function: crate::providers::ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({ "type": "object" }),
        },
    };

    let summaries = summarize_tools(&[tool]);

    assert_eq!(summaries[0].name, "sfumato_read_file");
    assert_eq!(summaries[0].description, "Read a file");
}
