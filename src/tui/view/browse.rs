//! Section listings, their action row, and the action prompt.

use super::*;

impl App {
    pub(super) fn draw_browse(&mut self, frame: &mut Frame<'_>, area: Rect, section: Section) {
        // One row for the actions, not a three-row box around one row of chips.
        let [actions_area, content_area] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(4)])
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
        frame.render_widget(Paragraph::new(Line::from(action_spans)), actions_area);
        // The list carries a title and a subtitle per row, so it needs room to show
        // them: at 40% the subtitles were cut mid-URL, which is where the useful part
        // of a connector's endpoint is. The detail pane wraps, so it loses less.
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
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
                .block(
                    Block::new()
                        .title(format!(" {} ", section.title()))
                        .title_style(Style::default().fg(CYAN).bold())
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(PANEL)),
                ),
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
                .block(
                    Block::new()
                        .title(" DETAIL ")
                        .title_style(Style::default().fg(CYAN).bold())
                        .borders(Borders::TOP | Borders::LEFT)
                        .border_style(Style::default().fg(PANEL))
                        .padding(Padding::left(1)),
                ),
            detail_area,
        );
    }

    pub(super) fn draw_operation(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(operation) = &self.operation else {
            return;
        };
        let width = area.width.saturating_sub(8).min(72);
        let height = (operation.fields.len() as u16 + 3)
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
        // The same renderer the forms use. This modal had its own copy of the field
        // rendering, in the bordered style the forms have moved away from, so a field
        // type added to one was invisible in the other.
        draw_resource_form(frame, content, &operation.fields, operation.selected);
    }
}
