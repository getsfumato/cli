//! Section listings, their action row, and the action prompt.

use super::*;

impl App {
    pub(super) fn draw_browse(&mut self, frame: &mut Frame<'_>, area: Rect, section: Section) {
        let [actions_area, content_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(5)])
                .areas(area.inner(Margin::new(2, 0)));
        let action_spans = section_actions(section)
            .iter()
            .enumerate()
            .flat_map(|(index, action)| {
                let selected =
                    self.browse_focus == BrowseFocus::Actions && index == self.browse_action_index;
                [
                    Span::styled(
                        format!(" {} ", action.label()),
                        if selected {
                            Style::default().fg(BG).bg(ACCENT).bold()
                        } else {
                            Style::default().fg(TEXT).bg(PANEL)
                        },
                    ),
                    Span::raw(" "),
                ]
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Line::from(action_spans)).block(panel("ACTIONS")),
            actions_area,
        );
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(content_area);
        let items = if self.browse_rows.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "No entries",
                Style::default().fg(MUTED),
            )))]
        } else {
            self.browse_rows
                .iter()
                .map(|row| {
                    let marker = if row.active { "*" } else { " " };
                    ListItem::new(vec![
                        Line::from(vec![
                            Span::styled(format!("{marker} "), Style::default().fg(GREEN)),
                            Span::styled(&row.title, Style::default().fg(TEXT).bold()),
                        ]),
                        Line::from(Span::styled(
                            format!("  {}", row.subtitle),
                            Style::default().fg(MUTED),
                        )),
                    ])
                })
                .collect()
        };
        let mut state = ListState::default()
            .with_selected((!self.browse_rows.is_empty()).then_some(self.browse_index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(PANEL))
                .block(panel(section.title())),
            list_area,
            &mut state,
        );
        let detail = self
            .browse_rows
            .get(self.browse_index)
            .map(|row| row.detail.as_str())
            .unwrap_or("Nothing selected");
        frame.render_widget(
            Paragraph::new(detail)
                .style(Style::default().fg(TEXT))
                .wrap(Wrap { trim: false })
                .scroll((self.browse_detail_scroll, 0))
                .block(panel("DETAIL")),
            detail_area,
        );
    }

    pub(super) fn draw_operation(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(operation) = &self.operation else {
            return;
        };
        let width = area.width.saturating_sub(8).min(72);
        let height = (operation.fields.len() as u16 * 3 + 2)
            .min(area.height.saturating_sub(2))
            .max(5);
        let modal = centered_rect(width, height, area);
        frame.render_widget(Clear, modal);
        frame.render_widget(
            Block::new()
                .title(format!(" {} ", operation.title))
                .title_style(Style::default().fg(ACCENT).bold())
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(ACCENT))
                .style(Style::default().bg(BG)),
            modal,
        );
        let content = modal.inner(Margin::new(2, 1));
        let range = visible_field_range(&operation.fields, operation.selected, content.height);
        let visible_fields = &operation.fields[range.clone()];
        let rows = Layout::vertical(
            visible_fields
                .iter()
                .map(|field| Constraint::Length(field_height(field)))
                .collect::<Vec<_>>(),
        )
        .split(content);
        for (offset, (field, row)) in visible_fields.iter().zip(rows.iter()).enumerate() {
            let index = range.start + offset;
            let selected = index == operation.selected;
            match field {
                FormField::Text {
                    label,
                    value,
                    placeholder,
                    ..
                } => {
                    let text = if value.is_empty() {
                        Span::styled(*placeholder, Style::default().fg(MUTED))
                    } else {
                        Span::styled(value.as_str(), Style::default().fg(TEXT))
                    };
                    frame.render_widget(
                        Paragraph::new(text).block(field_block(label, selected)),
                        *row,
                    );
                }
                FormField::Toggle { label, value } => {
                    let symbol = if *value { "[x]" } else { "[ ]" };
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled(
                                symbol,
                                Style::default().fg(if *value { GREEN } else { MUTED }),
                            ),
                            Span::raw(" "),
                            Span::styled(*label, Style::default().fg(TEXT)),
                        ]))
                        .block(field_block("OPTION", selected)),
                        *row,
                    );
                }
                FormField::Select {
                    label,
                    options,
                    selected: choice,
                } => {
                    frame.render_widget(
                        Paragraph::new(select_line(options, *choice, row.width.saturating_sub(2)))
                            .block(field_block(label, selected)),
                        *row,
                    );
                }
                FormField::MultiSelect {
                    label,
                    options,
                    cursor,
                    selected: choices,
                } => {
                    frame.render_widget(
                        Paragraph::new(multi_select_line(options, *cursor, choices))
                            .block(field_block(label, selected)),
                        *row,
                    );
                }
                FormField::Submit { label } => {
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            *label,
                            Style::default().fg(if selected { BG } else { TEXT }).bold(),
                        ))
                        .alignment(Alignment::Center)
                        .style(if selected {
                            Style::default().bg(ACCENT)
                        } else {
                            Style::default().bg(PANEL)
                        })
                        .block(Block::new().borders(Borders::ALL)),
                        *row,
                    );
                }
            }
        }
    }
}
