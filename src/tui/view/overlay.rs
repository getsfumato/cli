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
            Overlay::Choice {
                target,
                query,
                selected,
            } => {
                let values = self.choice_values(*target);
                let labels: Vec<&str> = values.iter().map(|choice| choice.value.as_str()).collect();
                let results = palette::matches(&labels, query);
                let card = centered_rect(60, (results.len().min(8) as u16) + 5, area);
                frame.render_widget(Clear, card);
                let title = self.choice_label(*target);
                let block = Block::new()
                    .title(format!(" {title} "))
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
                        Span::styled("▌", Style::default().fg(ACCENT)),
                    ]),
                    Line::from(""),
                ];
                if labels.is_empty() {
                    // An empty list is a fact about the workspace, not a dead end:
                    // saying so beats an empty box the user cannot interpret.
                    lines.push(Line::from(Span::styled(
                        "  nothing configured for this field",
                        Style::default().fg(MUTED),
                    )));
                } else if results.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "  no match",
                        Style::default().fg(MUTED),
                    )));
                }
                for (index, label) in results.iter().take(8).enumerate() {
                    let chosen = index == *selected;
                    let detail = values
                        .iter()
                        .find(|choice| choice.value == *label)
                        .map(|choice| choice.detail.clone())
                        .unwrap_or_default();
                    lines.push(Line::from(vec![
                        Span::styled(
                            if chosen { "  › " } else { "    " },
                            Style::default().fg(ACCENT),
                        ),
                        Span::styled(
                            format!("{label:<22}"),
                            if chosen {
                                Style::default().fg(TEXT).bold()
                            } else {
                                Style::default().fg(TEXT)
                            },
                        ),
                        // The identifier alone is often not enough to recognise: a model
                        // profile is told apart by its connector and model.
                        Span::styled(
                            compact(&detail, inner.width.saturating_sub(28) as usize),
                            Style::default().fg(MUTED),
                        ),
                    ]));
                }
                lines.push(Line::from(Span::styled(
                    "  del clears · esc cancels",
                    Style::default().fg(PANEL),
                )));
                frame.render_widget(Paragraph::new(lines), inner);
            }
            Overlay::Quit => {
                let running = self.screen == Screen::Running;
                let card = centered_rect(52, if running { 7 } else { 6 }, area);
                frame.render_widget(Clear, card);
                let block = Block::new()
                    .title(" LEAVE SFUMATO ")
                    .title_style(Style::default().fg(RED).bold())
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(RED))
                    .style(Style::default().bg(BG));
                let inner = block.inner(card);
                frame.render_widget(block, card);
                let mut lines = vec![Line::from(Span::styled(
                    "  Close the session?",
                    Style::default().fg(TEXT).bold(),
                ))];
                if running {
                    // The cost of leaving is the run, so it is stated here rather
                    // than discovered after the terminal has already been restored.
                    lines.push(Line::from(Span::styled(
                        "  The running operation will be cancelled and",
                        Style::default().fg(MUTED),
                    )));
                    lines.push(Line::from(Span::styled(
                        "  its staged artifacts discarded.",
                        Style::default().fg(MUTED),
                    )));
                } else {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  y leaves · any other key stays",
                    Style::default().fg(PANEL),
                )));
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
