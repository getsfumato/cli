//! The jump palette and the key reference, drawn over the current screen.

use super::*;

impl App {
    /// Draws the palette or the help card over the current screen.
    pub(super) fn draw_overlay(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(overlay) = &self.overlay else {
            return;
        };
        match overlay {
            Overlay::Palette { query, selected } => {
                let labels = App::palette_labels();
                let results = palette::matches(&labels, query);
                let height = (results.len().min(8) as u16) + 4;
                let card = centered_rect(52, height, area);
                frame.render_widget(Clear, card);
                let block = Block::new()
                    .title(" JUMP TO ")
                    .title_style(Style::default().fg(ACCENT).bold())
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(ACCENT))
                    .style(Style::default().bg(BG));
                let inner = block.inner(card);
                frame.render_widget(block, card);

                let mut lines = vec![
                    Line::from(vec![
                        Span::styled("› ", Style::default().fg(ACCENT)),
                        Span::styled(query.clone(), Style::default().fg(TEXT)),
                        // A visible caret, so an empty query does not look inert.
                        Span::styled("▌", Style::default().fg(ACCENT)),
                    ]),
                    Line::from(""),
                ];
                if results.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "  no match",
                        Style::default().fg(MUTED),
                    )));
                }
                for (index, label) in results.iter().take(8).enumerate() {
                    let chosen = index == *selected;
                    lines.push(Line::from(vec![
                        Span::styled(
                            if chosen { "  › " } else { "    " },
                            Style::default().fg(ACCENT),
                        ),
                        Span::styled(
                            *label,
                            if chosen {
                                Style::default().fg(TEXT).bold()
                            } else {
                                Style::default().fg(MUTED)
                            },
                        ),
                    ]));
                }
                frame.render_widget(Paragraph::new(lines), inner);
            }
            Overlay::Help => {
                let hints = self.key_hints();
                let card = centered_rect(44, hints.len() as u16 + 4, area);
                frame.render_widget(Clear, card);
                let block = Block::new()
                    .title(" KEYS ")
                    .title_style(Style::default().fg(CYAN).bold())
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(CYAN))
                    .style(Style::default().bg(BG));
                let inner = block.inner(card);
                frame.render_widget(block, card);
                let mut lines = Vec::new();
                for (key, action) in hints {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {key:<10}"), Style::default().fg(TEXT).bold()),
                        Span::styled(action, Style::default().fg(MUTED)),
                    ]));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  any key to close",
                    Style::default().fg(PANEL),
                )));
                frame.render_widget(Paragraph::new(lines), inner);
            }
        }
    }
}
