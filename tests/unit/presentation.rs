use super::*;

#[test]
fn renders_aligned_ascii_table_without_terminal_colors() {
    let rendered = render_table(
        &["NAME", "STATUS"],
        &[
            vec![Cell::primary("codex"), Cell::success("active")],
            vec![Cell::new("openrouter"), Cell::muted("configured")],
        ],
        false,
    );

    assert_eq!(
        rendered,
        "+------------+------------+\n\
         | NAME       | STATUS     |\n\
         +------------+------------+\n\
         | codex      | active     |\n\
         | openrouter | configured |\n\
         +------------+------------+"
    );
}

#[test]
fn terminal_colors_do_not_change_column_alignment() {
    let plain = render_table(&["NAME"], &[vec![Cell::warning("codex")]], false);
    let colored = render_table(&["NAME"], &[vec![Cell::warning("codex")]], true);

    assert!(colored.contains("\u{1b}[1;33mcodex\u{1b}[0m"));
    assert_eq!(plain.lines().map(str::len).collect::<Vec<_>>(), vec![9; 5]);
}
