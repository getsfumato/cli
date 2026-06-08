use super::*;

#[test]
fn filters_supported_files() {
    assert!(is_supported(Path::new("note.md")));
    assert!(is_supported(Path::new("main.RS")));
    assert!(!is_supported(Path::new("image.png")));
}

#[test]
fn strips_markdown_code_fence() {
    let text = "```markdown\n---\nmarp: true\n---\n# Title\n```";
    assert!(strip_code_fence(text).starts_with("---"));
}

#[test]
fn rejects_paths_outside_output_root() {
    let root = Path::new("/tmp/vault/out");
    let outside = Path::new("/tmp/vault/elsewhere/slides.md");
    assert!(ensure_inside(root, outside).is_err());
}

#[test]
fn normalizes_frontmatter() {
    let config = SfumatoConfig::default_for_cwd(PathBuf::from("/tmp/vault"));
    let markdown = normalize_marp_markdown("# Demo\n\n---\n\n## One", &config, "Demo").unwrap();

    assert!(markdown.contains("marp: true"));
    assert!(markdown.contains("theme: default"));
}
