use std::{collections::BTreeMap, fs};

use super::{StandalonePageAssembler, contains_html_tag};
use sfumato_core::{
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
        },
        guidance: String::new(),
        runtime_javascript: "window.pluginLoaded = true;".into(),
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
async fn bundled_plugins_execute_offline_in_a_real_browser() {
    use sfumato_core::{
        operation::OperationContext, page_plugins::PagePluginCatalog, renderers::PageInspector,
    };

    use crate::{page_plugins::BundledPagePluginCatalog, pages::ChromiumPageInspector};

    let (_directory, theme) = theme();
    let catalog = BundledPagePluginCatalog;
    let plugins = catalog
        .resolve(&[
            "threejs".into(),
            "motion".into(),
            "theatre".into(),
            "lottie".into(),
        ])
        .unwrap();
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
