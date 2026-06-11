use super::*;
use crate::config::GlobalConfig;

fn effective_config() -> EffectiveConfig {
    let global = GlobalConfig::default_config();
    EffectiveConfig {
        user: global.user,
        project_name: "demo".to_string(),
        project_root: PathBuf::from("/tmp/demo"),
        output_dir: PathBuf::from("Resources/Sfumato"),
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
    let config = effective_config();
    let markdown = normalize_marp_markdown("# Demo\n\n---\n\n## One", &config, "Demo").unwrap();
    assert!(markdown.contains("marp: true"));
}
