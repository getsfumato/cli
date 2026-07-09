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
    assert!(markdown.contains("math: mathjax"));
}

#[test]
fn normalizes_existing_math_frontmatter_to_mathjax() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\nmath: katex\n---\n\n# Demo\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.contains("math: mathjax"));
    assert!(!markdown.contains("math: katex"));
}

#[test]
fn removes_raw_svg_blocks_from_generated_decks() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "# Demo\n\n---\n\n<svg width=\"100\"><path d=\"M0 0\" /></svg>\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(!markdown.to_lowercase().contains("<svg"));
    assert!(!markdown.to_lowercase().contains("<path"));
}

#[test]
fn removes_common_html_wrapper_tags_from_generated_decks() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "# Demo\n\n---\n\n<section><div>Key idea</div></section>",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.contains("Key idea"));
    assert!(!markdown.to_lowercase().contains("<section"));
    assert!(!markdown.to_lowercase().contains("<div"));
}

#[test]
fn extracts_mermaid_fences_for_pre_rendering() {
    let markdown = "# Demo\n\n```mermaid\ngraph TD\n  A-->B\n```\n\nAfter";
    let blocks = extract_mermaid_blocks(markdown).unwrap();

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].source, "graph TD\n  A-->B");
    assert_eq!(
        &markdown[blocks[0].start..blocks[0].end],
        "```mermaid\ngraph TD\n  A-->B\n```"
    );
}

#[test]
fn embeds_rendered_svg_as_local_markdown_image() {
    let markdown = embedded_svg_markdown("demo", 2);

    assert_eq!(
        markdown,
        "![Mermaid diagram 2](diagrams/demo-diagram-2.svg)"
    );
}

#[test]
fn maps_theme_tokens_to_mermaid_theme_variables() {
    let config = mermaid_theme_config(&crate::themes::ThemeTokens {
        colors: BTreeMap::from([
            ("background".to_string(), "#fbf1c7".to_string()),
            ("surface".to_string(), "#f9f5d7".to_string()),
            ("surface-alt".to_string(), "#ebdbb2".to_string()),
            ("text".to_string(), "#3c3836".to_string()),
            ("primary".to_string(), "#9d0006".to_string()),
            ("accent".to_string(), "#af3a03".to_string()),
        ]),
        fonts: BTreeMap::from([("body".to_string(), "Inter, sans-serif".to_string())]),
    });
    let rendered = serde_json::to_string(&config).unwrap();

    assert!(rendered.contains("\"theme\":\"base\""));
    assert!(rendered.contains("\"primaryBorderColor\":\"#9d0006\""));
    assert!(rendered.contains("\"lineColor\":\"#af3a03\""));
    assert!(rendered.contains("\"fontFamily\":\"Inter, sans-serif\""));
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

#[test]
fn prompt_mentions_allowed_filesystem_root() {
    let config = effective_config();
    let package = ThemePackage {
        root: PathBuf::from("/tmp/theme"),
        manifest: crate::themes::ThemeManifest {
            schema_version: crate::themes::THEME_SCHEMA_VERSION,
            name: "sfumato-default".to_string(),
            description: "Demo".to_string(),
            tokens: Default::default(),
            adapters: crate::themes::ThemeAdapters {
                marp_css: PathBuf::from("theme.css"),
                html: None,
            },
        },
    };

    let request = build_generation_request(&config, &package, "Explain", "Explain", "");

    assert!(
        request
            .user_prompt
            .contains("Allowed filesystem root: /tmp/demo")
    );
    assert!(request.user_prompt.contains("prefer absolute paths"));
    assert!(request.user_prompt.contains("Explore selectively"));
}

#[test]
fn model_tool_rounds_uses_profile_option_or_default() {
    let mut profile = crate::config::ModelProfile {
        connector: "openrouter".to_string(),
        model: "model".to_string(),
        capabilities: vec![crate::config::Capability::Text],
        options: Default::default(),
    };

    assert_eq!(model_tool_rounds(&profile), 8);

    profile
        .options
        .insert("max_tool_rounds".to_string(), toml::Value::Integer(12));

    assert_eq!(model_tool_rounds(&profile), 12);
}
