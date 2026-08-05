//! The workspace menu and its context panel.

use super::*;

impl App {
    pub(super) fn draw_home(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let inner = area.inner(Margin::new(2, 0));
        // The menu carries its own group headings, so it needs no border. The right
        // column only appears when there is width for it; below that the workspace
        // facts move into the header, which already carries the project.
        // The menu's own widest row is 4 + 15 + 27 = 46 columns, so the side panel
        // only appears when both fit without the hints bleeding into it. Below that
        // the workspace facts are still in the header, which carries the project.
        const MENU_WIDTH: u16 = 46;
        const CONTEXT_WIDTH: u16 = 34;
        let (menu_area, context_area) = if inner.width >= MENU_WIDTH + CONTEXT_WIDTH {
            let [menu, context] = Layout::horizontal([
                Constraint::Min(MENU_WIDTH),
                Constraint::Length(CONTEXT_WIDTH),
            ])
            .areas(inner);
            (menu, Some(context))
        } else {
            (inner, None)
        };

        let mut lines = Vec::new();
        let mut row_of_item = Vec::new();
        let mut group = None;
        for (index, item) in NAV_ITEMS.iter().enumerate() {
            if group != Some(item.group) {
                if group.is_some() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(Span::styled(
                    item.group.title(),
                    Style::default().fg(CYAN).bold(),
                )));
                group = Some(item.group);
            }
            let selected = index == self.nav_index;
            row_of_item.push(lines.len());
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "  › " } else { "    " },
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    format!("{:<15}", item.title),
                    if selected {
                        Style::default().fg(TEXT).bold()
                    } else {
                        Style::default().fg(TEXT)
                    },
                ),
                // Truncated to what is left, so a long hint can never run into the
                // panel beside it.
                Span::styled(
                    compact(
                        item.hint,
                        menu_area.width.saturating_sub(19).max(1) as usize,
                    ),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), menu_area);

        let Some(context_area) = context_area else {
            return;
        };
        let mut context = vec![Line::from(Span::styled(
            "WORKSPACE",
            Style::default().fg(CYAN).bold(),
        ))];
        for (label, value) in [
            ("projects", self.snapshot.projects),
            ("models", self.snapshot.models),
            ("connectors", self.snapshot.connectors),
            ("themes", self.snapshot.themes),
        ] {
            context.push(Line::from(vec![
                Span::styled(format!("{value:>4}  "), Style::default().fg(TEXT).bold()),
                Span::styled(label, Style::default().fg(MUTED)),
            ]));
        }
        if let Some(project) = &self.snapshot.project {
            context.push(Line::from(""));
            context.push(Line::from(Span::styled(
                "ACTIVE PROJECT",
                Style::default().fg(CYAN).bold(),
            )));
            context.push(Line::from(Span::styled(
                project.name.clone(),
                Style::default().fg(TEXT).bold(),
            )));
            context.push(Line::from(Span::styled(
                project.path.display().to_string(),
                Style::default().fg(MUTED),
            )));
        }
        // Problems reach the screen instead of being swallowed: the previous code
        // used `.ok()` per call, so a broken registry rendered as zero of everything.
        if let Some(hint) = self.snapshot.project_hint() {
            context.push(Line::from(""));
            context.push(Line::from(Span::styled(hint, Style::default().fg(ACCENT))));
        }
        for problem in self.snapshot.problems.iter().take(3) {
            context.push(Line::from(Span::styled(
                compact(problem, context_area.width as usize),
                Style::default().fg(RED),
            )));
        }
        frame.render_widget(
            Paragraph::new(context).wrap(Wrap { trim: true }),
            context_area,
        );
    }
}
