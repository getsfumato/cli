use std::{collections::BTreeMap, fs};

use super::{
    StandalonePageAssembler, contains_html_tag, declared_custom_properties, page_inspection_error,
    undeclared_token_properties,
};
use sfumato_core::{
    errors::ErrorClass,
    page_plugins::{PagePluginPackage, PagePluginSummary},
    renderers::{PageAssembler, PageAssemblyRequest},
    resources::pages::PageDocument,
    themes::{HtmlThemeAdapter, ThemeAdapters, ThemeManifest, ThemePackage, ThemeTokens},
};

fn theme() -> (tempfile::TempDir, ThemePackage) {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("html")).unwrap();
    fs::write(
        directory.path().join("html/page.html"),
        "<!doctype html><html><head><link rel=\"stylesheet\" href=\"style.css\"><title>Old</title></head><body><main><!-- SFUMATO_CONTENT --></main><script src=\"script.js\"></script></body></html>",
    ).unwrap();
    fs::write(
        directory.path().join("html/style.css"),
        "body { color: #222; }",
    )
    .unwrap();
    fs::write(
        directory.path().join("html/script.js"),
        "window.themeLoaded = true;",
    )
    .unwrap();
    let package = ThemePackage {
        root: directory.path().to_path_buf(),
        manifest: ThemeManifest {
            schema_version: 1,
            name: "test".into(),
            description: "test".into(),
            tokens: ThemeTokens {
                colors: BTreeMap::new(),
                fonts: BTreeMap::new(),
            },
            adapters: ThemeAdapters {
                marp_css: "marp.css".into(),
                html: Some(HtmlThemeAdapter {
                    shell: "html/page.html".into(),
                    css: "html/style.css".into(),
                    script: Some("html/script.js".into()),
                }),
                document: None,
            },
        },
    };
    (directory, package)
}

#[test]
fn assembles_theme_plugins_and_page_in_stable_order() {
    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Fourier Explorer",
        "<section><h1>Fourier</h1></section>",
        "#sfumato-page { display: grid; }",
        "window.pageLoaded = true;",
    )
    .unwrap();
    let plugin = PagePluginPackage {
        summary: PagePluginSummary {
            id: "threejs".into(),
            name: "Three.js".into(),
            version: "1".into(),
            api_global: "window.SfumatoPlugins.threejs".into(),
            runtime_hash: "hash".into(),
            license: "MIT".into(),
            category: sfumato_core::page_plugins::PagePluginCategory::Utility,
            dependencies: Vec::new(),
        },
        guidance: String::new(),
        runtime_javascript: "window.pluginLoaded = true;".into(),
        stylesheet: ".plugin { color: red; }".into(),
    };
    let assembled = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[plugin],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap();
    let html = assembled.html;
    assert!(assembled.runtimes.is_empty());
    assert!(html.contains("Content-Security-Policy"));
    assert!(html.contains("<title>Fourier Explorer</title>"));
    assert!(!html.contains("href=\"style.css\""));
    assert!(!html.contains("src=\"script.js\""));
    assert!(html.find("pluginLoaded").unwrap() < html.find("themeLoaded").unwrap());
    assert!(html.find("themeLoaded").unwrap() < html.find("pageLoaded").unwrap());
}

#[test]
fn rejects_remote_and_unregistered_assets() {
    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Unsafe",
        "<img src=\"https://example.com/image.png\">",
        "",
        "",
    )
    .unwrap();
    let error = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap_err();
    assert!(error.to_string().contains("remote URLs"));
}

#[test]
fn accepts_a_registered_local_video_asset_and_offline_media_csp() {
    let (_directory, theme) = theme();
    let asset_directory = tempfile::tempdir().unwrap();
    let video = asset_directory.path().join("lesson.mp4");
    fs::write(&video, b"test-video").unwrap();
    let page = PageDocument::new(
        "Animated lesson",
        r#"<section><video controls preload="metadata" src="assets/videos/lesson.mp4"></video></section>"#,
        "video { max-width: 100%; height: auto; }",
        "",
    )
    .unwrap();

    let html = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[video],
            inspection: false,
        })
        .unwrap()
        .html;

    assert!(html.contains("media-src 'self' data:"));
    assert!(html.contains("assets/videos/lesson.mp4"));
}

#[test]
fn accepts_semantic_header_without_confusing_it_with_head() {
    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Semantic header",
        "<header><h1>Fourier Explorer</h1></header><section>Content</section>",
        "",
        "",
    )
    .unwrap();

    let html = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap()
        .html;

    assert!(html.contains("<header><h1>Fourier Explorer</h1></header>"));
}

#[test]
fn automatically_embeds_the_pinned_mathjax_runtime_for_tex() {
    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Fourier formula",
        r#"<section><h1>Fourier</h1><p>\[f(t)=\sum_{n=1}^{\infty}a_n\cos(nt)\]</p></section>"#,
        "",
        "",
    )
    .unwrap();

    let assembled = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap();

    assert_eq!(assembled.runtimes.len(), 1);
    assert_eq!(assembled.runtimes[0].id, "mathjax");
    assert_eq!(assembled.runtimes[0].version, "3.2.2");
    assert_eq!(assembled.runtimes[0].runtime_hash.len(), 64);
    assert!(assembled.html.contains("data-sfumato-runtime=\"mathjax\""));
    assert!(assembled.html.contains("data-sfumato-math-config"));
    assert!(assembled.html.contains("mjx-container[jax=\"SVG\"]"));
}

#[test]
fn rejects_actual_document_head_elements() {
    assert!(contains_html_tag(
        "<head><title>Invalid</title></head>",
        "head"
    ));
    assert!(!contains_html_tag("<header>Valid</header>", "head"));

    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Invalid shell",
        "<head><meta charset=\"utf-8\"></head><section>Content</section>",
        "",
        "",
    )
    .unwrap();

    StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap_err();
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn installed_plugins_execute_offline_in_a_real_browser() {
    use sfumato_core::{
        operation::OperationContext, page_plugins::PagePluginCatalog, renderers::PageInspector,
    };

    use crate::{page_plugins::FilesystemPagePluginCatalog, pages::ChromiumPageInspector};

    let (_directory, theme) = theme();
    let catalog = FilesystemPagePluginCatalog::default_path().unwrap();
    let Ok(plugins) = catalog.resolve(&[
        "threejs".into(),
        "motion".into(),
        "theatre".into(),
        "lottie".into(),
    ]) else {
        return;
    };
    let page = PageDocument::new(
        "Offline plugin check",
        "<section><h1>Offline plugin check</h1><p id=\"status\">pending</p></section>",
        "#sfumato-page { max-width: 60rem; margin: auto; }",
        "const p = window.SfumatoPlugins; if (!p.threejs || !p.motion || !p.theatre || !p.lottie) throw new Error('missing plugin'); document.getElementById('status').textContent = 'ready';",
    ).unwrap();
    let html = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &plugins,
            allowed_assets: &[],
            inspection: true,
        })
        .unwrap()
        .html;
    let directory = tempfile::tempdir().unwrap();
    let html_path = directory.path().join("index.html");
    fs::write(&html_path, html).unwrap();
    let issues = ChromiumPageInspector
        .inspect(&html_path, None, &OperationContext::detached())
        .await
        .unwrap();
    assert!(issues.is_empty(), "{issues:#?}");
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn bundled_mathjax_renders_tex_to_svg_offline() {
    use sfumato_core::{operation::OperationContext, renderers::PageInspector};

    use crate::pages::ChromiumPageInspector;

    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "MathJax offline check",
        r#"<section><h1>Fourier</h1><p>\[f(t)=\sum_{n=1}^{\infty}a_n\cos(nt)\]</p></section>"#,
        "",
        "",
    )
    .unwrap();
    let assembled = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: true,
        })
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let html_path = directory.path().join("index.html");
    fs::write(&html_path, assembled.html).unwrap();

    let issues = ChromiumPageInspector
        .inspect(&html_path, None, &OperationContext::detached())
        .await
        .unwrap();

    assert!(issues.is_empty(), "{issues:#?}");
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn material_ui_and_react_execute_offline_in_a_real_browser() {
    use sfumato_core::{
        operation::OperationContext, page_plugins::PagePluginCatalog, renderers::PageInspector,
    };

    use crate::{page_plugins::FilesystemPagePluginCatalog, pages::ChromiumPageInspector};

    let (_directory, theme) = theme();
    let Ok(plugins) = FilesystemPagePluginCatalog::default_path()
        .unwrap()
        .resolve(&["materialui".into()])
    else {
        return;
    };
    let page = PageDocument::new(
        "Material UI offline check",
        "<main><div id=\"sfumato-react-root\"><p>Loading lesson...</p></div></main>",
        "#sfumato-react-root { min-height: 12rem; }",
        "const e = React.createElement; const {Button, Stack, Typography} = MaterialUI; const app = e(Stack, {spacing: 2}, e(Typography, {variant: 'h4'}, 'Fourier'), e(Button, {variant: 'contained', id: 'ready'}, 'Ready')); ReactDOM.createRoot(document.getElementById('sfumato-react-root')).render(app);",
    )
    .unwrap();
    let html = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &plugins,
            allowed_assets: &[],
            inspection: true,
        })
        .unwrap()
        .html;
    let directory = tempfile::tempdir().unwrap();
    let html_path = directory.path().join("index.html");
    fs::write(&html_path, html).unwrap();

    let issues = ChromiumPageInspector
        .inspect(&html_path, None, &OperationContext::detached())
        .await
        .unwrap();

    assert!(issues.is_empty(), "{issues:#?}");
}

#[test]
fn a_remote_reference_is_quoted_back_so_a_repair_can_target_it() {
    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Unsafe",
        "<div id=\"chart\"></div>",
        "",
        "const chart = 'https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.js';",
    )
    .unwrap();

    let error = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap_err()
        .to_string();

    // Saying only that a remote URL exists somewhere leaves the repair pass guessing,
    // and it gets one attempt.
    assert!(
        error.contains("https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.js"),
        "the offending URL must appear in the error: {error}"
    );
    assert!(
        error.contains("images/"),
        "the error must say where local assets belong: {error}"
    );
}

#[test]
fn several_remote_references_are_all_reported_and_a_flood_is_summarised() {
    let many = (0..12)
        .map(|index| format!("const url{index} = 'https://cdn.example.com/{index}.js';"))
        .collect::<Vec<_>>()
        .join("\n");
    let (_directory, theme) = theme();
    let page = PageDocument::new("Unsafe", "<div></div>", "", &many).unwrap();

    let error = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("https://cdn.example.com/0.js"), "{error}");
    // A field full of URLs must not turn one validation error into a wall of text the
    // repair prompt has to wade through.
    assert!(error.contains("and more"), "{error}");
    assert!(
        !error.contains("https://cdn.example.com/11.js"),
        "the list is capped: {error}"
    );
}

#[test]
fn a_traversal_reference_is_quoted_back_too() {
    let (_directory, theme) = theme();
    let page = PageDocument::new("Unsafe", "<img src=\"../../secrets/key.png\">", "", "").unwrap();

    let error = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("traverse outside"), "{error}");
    assert!(error.contains("../"), "{error}");
}

#[test]
fn scripted_svg_is_accepted_because_a_namespace_is_not_a_fetch() {
    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Diagram",
        "<div id=\"plot\"></div>",
        "",
        // The only way the DOM makes an SVG element. The page prompt now actively steers
        // the model here — "build interactivity with plain DOM, CSS, SVG, or canvas" —
        // so rejecting it turned the advice into a guaranteed validation failure.
        "const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');\n\
         svg.setAttributeNS('http://www.w3.org/2000/xmlns/', 'xmlns', 'http://www.w3.org/2000/svg');\n\
         document.getElementById('plot').append(svg);",
    )
    .unwrap();

    StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .expect("an xmlns declaration is not a remote reference");
}

#[test]
fn a_real_cdn_beside_a_namespace_is_still_refused() {
    let (_directory, theme) = theme();
    let page = PageDocument::new(
        "Mixed",
        "<div></div>",
        "",
        "const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');\n\
         const lib = 'https://cdn.jsdelivr.net/npm/d3';",
    )
    .unwrap();

    let error = StandalonePageAssembler
        .assemble(PageAssemblyRequest {
            document: &page,
            theme: &theme,
            plugins: &[],
            allowed_assets: &[],
            inspection: false,
        })
        .unwrap_err()
        .to_string();

    // Allowing namespaces must not blunt the check that matters.
    assert!(error.contains("https://cdn.jsdelivr.net/npm/d3"), "{error}");
    assert!(
        !error.contains("2000/svg"),
        "the namespace must not be listed as an offender: {error}"
    );
}

#[test]
fn a_theme_installed_before_the_token_vocabulary_still_gets_it() {
    // Written once at import and never rewritten, so a theme on disk from an older build
    // ships only the six aliases. The page then references `var(--canvas)` — which the
    // prompt told it to use — and gets nothing.
    let stale = ":root { --background: #ffffff; --text: #202124; }\n";
    let (_directory, mut package) = theme();
    for (name, value) in [("canvas", "#181818"), ("ink", "#ffffff")] {
        package
            .manifest
            .tokens
            .colors
            .insert(name.into(), value.into());
    }
    let manifest = package.manifest;

    let filled = undeclared_token_properties(stale, &manifest);

    assert!(filled.contains("--canvas: #181818"), "{filled}");
    assert!(filled.contains("--ink: #ffffff"), "{filled}");
    // What the stylesheet does declare stays authoritative: it may have been tuned.
    assert!(!filled.contains("--background"), "{filled}");
    assert!(!filled.contains("--text:"), "{filled}");
}

#[test]
fn a_current_theme_needs_no_second_block_at_all() {
    let (_directory, mut package) = theme();
    package
        .manifest
        .tokens
        .colors
        .insert("canvas".into(), "#181818".into());
    let manifest = package.manifest;
    let complete = ":root { --canvas: #181818; }\n";

    assert_eq!(undeclared_token_properties(complete, &manifest), "");
}

#[test]
fn using_a_variable_is_not_declaring_it() {
    // `var(--canvas)` is a use; only `--canvas:` claims the name. Confusing the two would
    // let a page's own reference suppress the definition it needs.
    let uses_only = "body { background: var(--canvas); color: var(--ink); }\n";

    let declared = declared_custom_properties(uses_only);

    assert!(declared.is_empty(), "{declared:?}");
}

#[test]
fn a_missing_browser_is_reported_as_unavailable_not_permanent() {
    // Same string agreement as the slide and document renderers: the class is
    // decided by matching the message, so it needs a test that fails when the
    // message changes shape.
    let error = page_inspection_error(anyhow::anyhow!(crate::browser::not_found(
        "for page inspection"
    )));
    assert_eq!(error.class, ErrorClass::Unavailable);
}
