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
        model_roles: global.model_roles,
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
fn strips_marp_code_fence_without_leaving_the_language_label() {
    let text = "```marp\n---\nmarp: true\n---\n# Title\n```";
    let stripped = strip_code_fence(text);

    assert!(stripped.starts_with("---"));
    assert!(!stripped.starts_with("marp\n"));
}

#[test]
fn normalizes_a_fenced_marp_deck_without_metadata_slides() {
    let mut config = effective_config();
    config.theme = "gruvbox".to_string();
    let generated = "```marp\n---\nmarp: true\ntheme: gruvbox\nmath: mathjax\npaginate: true\n---\n\n<!-- _class: lead -->\n\n# Fourier Series\n\n---\n\n## Intuition\n\nContent.\n```";
    let markdown = normalize_marp_markdown(generated, &config, "Fourier Series").unwrap();

    assert_eq!(markdown.matches("marp: true").count(), 1);
    assert_eq!(markdown.matches("# Fourier Series").count(), 1);
    assert!(!markdown.contains("\n\nmarp\n\n---"));
    assert!(!markdown.contains("\n\nmarp: true\ntheme: gruvbox"));
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
fn promotes_late_marp_frontmatter_and_preserves_title_slide() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\n\n# Demo\n\n---\nmarp: true\npaginate: true\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.starts_with("---\n"));
    assert!(markdown.contains("marp: true"));
    assert!(markdown.contains("\n\n# Demo\n\n---\n\n## One"));
    assert!(!markdown.starts_with("---\n\n# Demo\n\n---\nmarp: true"));
}

#[test]
fn inserts_missing_title_after_canonical_frontmatter() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n## One\n\n---\n\n## Two",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.starts_with(
        "---\nmarp: true\ntheme: sfumato-default\npaginate: true\nmath: mathjax\n---\n\n# Demo\n\n---\n\n## One"
    ));
    assert!(!markdown.starts_with("---\n\n# Demo\n\n---\nmarp: true"));
}

#[test]
fn removes_generated_frontmatter_css() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\nstyle: |\n  section { color: red; }\n  h1 { color: blue; }\nmath: katex\ncustom: ignored\n---\n\n# Demo\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(markdown.starts_with(
        "---\nmarp: true\ntheme: sfumato-default\npaginate: true\nmath: mathjax\n---"
    ));
    assert!(!markdown.contains("style:"));
    assert!(!markdown.contains("section { color: red; }"));
    assert!(!markdown.contains("custom: ignored"));
    assert!(markdown.contains("math: mathjax"));
    assert!(!markdown.contains("math: katex"));
}

#[test]
fn removes_generated_css_blocks_from_slide_body() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n<style>\nsection { color: red; }\n</style>\n\n```css\n@theme bad\nsection { color: blue; }\n```\n\n# Demo\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(!markdown.contains("<style"));
    assert!(!markdown.contains("@theme bad"));
    assert!(!markdown.contains("color: blue"));
    assert!(markdown.contains("# Demo"));
}

#[test]
fn removes_duplicate_leading_title_only_slides() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n# Demo\n\n---\n\n<!-- _class: title -->\n\n# Demo\n\n## Real title slide\n\n---\n\n## One",
        &config,
        "Demo",
    )
    .unwrap();

    assert!(!markdown.contains("# Demo\n\n---\n\n<!-- _class: title -->"));
    assert!(markdown.contains("<!-- _class: title -->\n\n# Demo"));
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
fn normalizes_mermaid_labels_before_rendering() {
    let source = "flowchart TB\n    A[\"a₀/2average heightDC offset\"]";
    let normalized = normalize_mermaid_source(source);

    assert!(normalized.contains("a₀/2 average height DC<br/>offset"));
}

#[test]
fn preserves_existing_mermaid_label_breaks() {
    let source = "flowchart TB\n    A[\"first line\\nsecond line\"]";
    let normalized = normalize_mermaid_source(source);

    assert!(normalized.contains("first line<br/>second line"));
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

    let request = build_generation_request(&config, &package, "Explain", None, "", false);

    assert!(
        request
            .user_prompt
            .contains("Allowed filesystem root: /tmp/demo")
    );
    assert!(request.user_prompt.contains("prefer absolute paths"));
    assert!(request.user_prompt.contains("Explore selectively"));
    assert!(
        request
            .user_prompt
            .contains("Do not copy or lightly rephrase the instruction")
    );
    assert!(request.user_prompt.contains("first `# H1`"));
}

#[test]
fn generation_prompt_preserves_an_explicit_title() {
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

    let request = build_generation_request(
        &config,
        &package,
        "Explain this subject",
        Some("A Deliberate Title"),
        "",
        false,
    );

    assert!(
        request
            .user_prompt
            .contains("Use exactly \"A Deliberate Title\" as the deck title")
    );
    assert!(
        request
            .user_prompt
            .contains("Instruction: Explain this subject")
    );
}

#[test]
fn generation_prompt_explains_how_to_embed_generated_images() {
    let config = effective_config();
    let package = ThemePackage {
        root: PathBuf::from("/tmp/theme"),
        manifest: crate::themes::ThemeManifest {
            schema_version: crate::themes::THEME_SCHEMA_VERSION,
            name: "gruvbox".to_string(),
            description: "Demo".to_string(),
            tokens: Default::default(),
            adapters: crate::themes::ThemeAdapters {
                marp_css: PathBuf::from("theme.css"),
                html: None,
            },
        },
    };

    let request = build_generation_request(&config, &package, "Explain", None, "", true);

    assert!(request.user_prompt.contains("`sfumato_image_gen`"));
    assert!(request.user_prompt.contains("returned `markdown_path`"));
    assert!(request.user_prompt.contains("project theme automatically"));
}

#[test]
fn title_repair_prompt_requests_only_a_title_and_uses_deck_headings() {
    let config = effective_config();
    let request = build_title_repair_request(
        &config,
        "Explain Fourier series visually",
        "---\nmarp: true\n---\n\n## Periodic signals\n\n---\n\n## Harmonic spectrum",
        "The drafter did not provide a title",
    );

    assert!(request.system_prompt.contains("one plain-text line"));
    assert!(request.user_prompt.contains("Periodic signals"));
    assert!(request.user_prompt.contains("Harmonic spectrum"));
    assert!(request.user_prompt.contains("Do not regenerate"));
    assert!(request.tools.is_empty());
}

#[test]
fn parses_a_focused_title_repair_response() {
    let title = parse_repaired_title(
        "# From Periodic Signals to Harmonic Spectra\n",
        "Explain Fourier series visually",
    )
    .unwrap();

    assert_eq!(title, "From Periodic Signals to Harmonic Spectra");
}

#[test]
fn title_repair_rejects_the_original_instruction() {
    assert!(
        parse_repaired_title(
            "Explain Fourier series visually",
            "Explain Fourier series visually"
        )
        .is_err()
    );
}

#[test]
fn extracts_the_drafters_title_from_the_first_h1() {
    let generated = "---\nmarp: true\ntheme: demo\n---\n\n<!-- _class: lead -->\n\n# **Fourier Series: From Waves to Spectra**\n\n---\n\n## Intuition";

    assert_eq!(
        extract_generated_title(generated).as_deref(),
        Some("Fourier Series: From Waves to Spectra")
    );
}

#[test]
fn title_extraction_ignores_h1_examples_inside_code_fences() {
    let generated = "---\nmarp: true\n---\n\n```markdown\n# Not the deck title\n```\n\n# The Actual Deck Title\n\n---\n\n## One";

    assert_eq!(
        extract_generated_title(generated).as_deref(),
        Some("The Actual Deck Title")
    );
}

#[test]
fn title_extraction_rejects_a_deck_without_an_h1() {
    let generated = "---\nmarp: true\n---\n\n## First topic\n\n---\n\n## Second topic";

    assert_eq!(extract_generated_title(generated), None);
}

#[test]
fn recognizes_when_the_drafter_reuses_the_instruction_as_title() {
    assert!(titles_are_equivalent(
        "Explain Fourier Series!",
        "explain fourier series"
    ));
    assert!(!titles_are_equivalent(
        "Fourier Series: From Waves to Spectra",
        "Explain Fourier series visually"
    ));
}

#[test]
fn normalization_keeps_the_artifact_title_on_the_first_slide() {
    let config = effective_config();
    let markdown = normalize_marp_markdown(
        "---\nmarp: true\n---\n\n# Reviewer Changed It\n\n---\n\n## One",
        &config,
        "Fourier Series: A Visual Guide",
    )
    .unwrap();

    assert!(markdown.contains("\n\n# Fourier Series: A Visual Guide\n\n---"));
    assert!(!markdown.contains("Reviewer Changed It"));
}

#[test]
fn artifact_paths_use_the_generated_title_slug() {
    let (markdown, pdf) =
        slide_artifact_paths(Path::new("/tmp/slides"), "Fourier Series: A Visual Guide").unwrap();

    assert_eq!(
        markdown,
        PathBuf::from("/tmp/slides/fourier-series-a-visual-guide.md")
    );
    assert_eq!(
        pdf,
        PathBuf::from("/tmp/slides/fourier-series-a-visual-guide.pdf")
    );
}

#[test]
fn review_prompt_requests_targeted_json_patch_with_grounding() {
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
    let deck = SlideDeckDocument::from_marp(
        "---\nmarp: true\ntheme: sfumato-default\n---\n\n# Fourier series\n\n---\n\n## Definition",
        "Fourier series",
    )
    .unwrap();
    let snapshot = deck.snapshot().unwrap();
    let request = build_review_request(
        &config,
        &package,
        "Explain Fourier series",
        "SOURCE: notes.md",
        &snapshot,
        None,
    )
    .unwrap();

    assert!(request.user_prompt.contains("Explain Fourier series"));
    assert!(request.user_prompt.contains("SOURCE: notes.md"));
    assert!(request.user_prompt.contains("Definition"));
    assert!(request.user_prompt.contains("RFC 6902 JSON Patch"));
    assert!(request.user_prompt.contains("/slides/<id>/revision"));
    assert!(!request.user_prompt.contains("complete revised Marp"));
}

#[test]
fn review_retry_prompt_returns_the_validation_error_to_the_model() {
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
    let deck = SlideDeckDocument::from_marp(
        "---\nmarp: true\n---\n\n# Demo\n\n---\n\n## Diagram",
        "Demo",
    )
    .unwrap();
    let snapshot = deck.snapshot().unwrap();
    let retry = ReviewRetryContext {
        invalid_response: "[{\"op\":\"replace\"}]".to_string(),
        error: "Slide `slide-2` has an unclosed `mermaid` fenced code block".to_string(),
    };

    let request = build_review_request(
        &config,
        &package,
        "Explain diagrams",
        "",
        &snapshot,
        Some(&retry),
    )
    .unwrap();

    assert!(request.user_prompt.contains("Corrective retry"));
    assert!(request.user_prompt.contains("unclosed `mermaid`"));
    assert!(
        request
            .user_prompt
            .contains("previous response was rejected")
    );
    assert!(
        request
            .user_prompt
            .contains("against the original snapshot")
    );
}

#[test]
fn layout_repair_request_has_no_filesystem_tools() {
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
    let request = build_layout_repair_request(
        LayoutRepairRequestContext {
            config: &config,
            theme: &package,
            instruction: "Explain",
            title: "Explain",
            slide_markdown: "## Dense\n\n- Too much content",
            issue: &SlideLayoutIssue {
                slide: 2,
                title: "Dense".to_string(),
                vertical_overflow_px: 80,
                horizontal_overflow_px: 0,
            },
        },
        None,
        None,
    );

    assert!(request.tools.is_empty());
    assert!(request.user_prompt.contains("vertical_overflow_px"));
    assert!(request.user_prompt.contains("## Dense"));
    assert!(!request.user_prompt.contains("complete revised Marp"));
    assert!(
        request
            .user_prompt
            .contains("split this slide into two coherent slides")
    );
}

#[test]
fn layout_repair_retry_prompt_includes_mermaid_error_and_original_slide() {
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
    let retry = LayoutRepairRetryContext {
        invalid_response: "## Broken\n\n```mermaid\nA---B".to_string(),
        error: "Mermaid CLI parse error near A---B".to_string(),
    };
    let request = build_layout_repair_request(
        LayoutRepairRequestContext {
            config: &config,
            theme: &package,
            instruction: "Explain",
            title: "A useful title",
            slide_markdown: "## Original slide\n\nValid content.",
            issue: &SlideLayoutIssue {
                slide: 12,
                title: "Original slide".to_string(),
                vertical_overflow_px: 40,
                horizontal_overflow_px: 0,
            },
        },
        Some(&retry),
        None,
    );

    assert!(request.user_prompt.contains("Corrective retry"));
    assert!(request.user_prompt.contains("Mermaid CLI parse error"));
    assert!(request.user_prompt.contains("A---B"));
    assert!(request.user_prompt.contains("## Original slide"));
    assert!(request.user_prompt.contains("original slide fragment"));
}

#[test]
fn model_output_retry_excludes_missing_dependencies() {
    assert!(should_retry_model_output(&anyhow::anyhow!(
        "Mermaid CLI exited with a parse error"
    )));
    assert!(!should_retry_model_output(&anyhow::anyhow!(
        "Mermaid CLI is not installed"
    )));
}

#[test]
fn locates_slide_ranges_without_counting_separators_inside_code_fences() {
    let markdown = "---\nmarp: true\ntheme: demo\n---\n\n# One\n\n---\n\n## Two\n\n```yaml\n---\nvalue: true\n---\n```\n\n---\n\n## Three";
    let ranges = slide_ranges(markdown).unwrap();

    assert_eq!(ranges.len(), 3);
    assert_eq!(markdown[ranges[0].start..ranges[0].end].trim(), "# One");
    assert!(markdown[ranges[1].start..ranges[1].end].contains("```yaml"));
    assert_eq!(markdown[ranges[2].start..ranges[2].end].trim(), "## Three");
}

#[test]
fn focused_replacement_changes_only_the_target_slide() {
    let markdown = "---\nmarp: true\ntheme: demo\n---\n\n# One\n\n---\n\n## Dense\n\nOld content\n\n---\n\n## Three";
    let ranges = slide_ranges(markdown).unwrap();
    let repaired = apply_slide_replacements(
        markdown,
        vec![SlideReplacement {
            range: ranges[1].clone(),
            markdown: "## Dense, part one\n\nShort.\n\n---\n\n## Dense, part two\n\nShort."
                .to_string(),
        }],
    );

    assert!(repaired.contains("# One"));
    assert!(repaired.contains("## Three"));
    assert!(!repaired.contains("Old content"));
    assert!(repaired.contains("## Dense, part one"));
    assert!(repaired.contains("## Dense, part two"));
    assert_eq!(slide_ranges(&repaired).unwrap().len(), 4);
}

#[test]
fn applies_multiple_focused_replacements_without_offset_drift() {
    let markdown = "---\nmarp: true\ntheme: demo\n---\n\n# One\n\n---\n\n## Two\n\nOld two\n\n---\n\n## Three\n\nOld three";
    let ranges = slide_ranges(markdown).unwrap();
    let repaired = apply_slide_replacements(
        markdown,
        vec![
            SlideReplacement {
                range: ranges[1].clone(),
                markdown: "## Two\n\nNew two".to_string(),
            },
            SlideReplacement {
                range: ranges[2].clone(),
                markdown: "## Three\n\nNew three".to_string(),
            },
        ],
    );

    assert!(repaired.contains("New two"));
    assert!(repaired.contains("New three"));
    assert!(!repaired.contains("Old two"));
    assert!(!repaired.contains("Old three"));
}

#[test]
fn normalizes_reviewer_fragment_without_document_frontmatter() {
    let generated = "```markdown\n---\nmarp: true\ntheme: wrong\n---\n\n---\n\n## Fixed\n\nShort content.\n\n---\n```";
    let fragment = normalize_slide_replacement(generated).unwrap();

    assert_eq!(fragment, "## Fixed\n\nShort content.");
    assert!(!fragment.contains("marp: true"));
}

#[test]
fn layout_score_prioritizes_issue_count_then_overflow() {
    let one_issue = vec![SlideLayoutIssue {
        slide: 1,
        title: "One".to_string(),
        vertical_overflow_px: 100,
        horizontal_overflow_px: 0,
    }];
    let two_issues = vec![
        SlideLayoutIssue {
            slide: 1,
            title: "One".to_string(),
            vertical_overflow_px: 1,
            horizontal_overflow_px: 0,
        },
        SlideLayoutIssue {
            slide: 2,
            title: "Two".to_string(),
            vertical_overflow_px: 1,
            horizontal_overflow_px: 0,
        },
    ];

    assert!(layout_score(&one_issue) < layout_score(&two_issues));
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
