//! Ratatui rendering entry point and the chrome every screen shares.
//!
//! `draw` used to be one file holding a five-hundred-line function that covered
//! six screens, every helper they shared, and every helper only one of them used.
//! Each screen owns a module now; this one keeps the frame — the layout, the
//! header, the footer, and the pieces more than one screen draws with.

mod browse;
mod form;
mod home;
mod overlay;
mod run;

use super::*;

// Re-exported for the reducer and the tests, which reason about form geometry and
// stage names without rendering anything.
pub(super) use form::{field_height, multi_select_line, select_line, visible_field_range};
pub(super) use run::stage_label;

pub(super) fn draw(app: &mut App, frame: &mut Frame<'_>) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::default().bg(BG).fg(TEXT)), area);
    // One line of chrome, not six. The wordmark used to be rendered as a five-row
    // ASCII logo on every screen, which spent a fifth of an 80x24 terminal on
    // decoration that says the same thing each frame.
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .areas(area);
    app.draw_header(frame, header);
    match app.screen {
        Screen::Home => app.draw_home(frame, body),
        Screen::Browse(section) => app.draw_browse(frame, body, section),
        Screen::Generate => app.draw_generate(frame, body),
        Screen::Edit => app.draw_edit(frame, body),
        Screen::Running | Screen::Complete => app.draw_generation(frame, body),
    }
    if app.operation.is_some() {
        app.draw_operation(frame, body);
    }
    // Last, so it sits over whatever screen launched it.
    if app.overlay.is_some() {
        app.draw_overlay(frame, area);
    }
    app.draw_footer(frame, footer);
    app.effects
        .process_effects(TICK_RATE.into(), frame.buffer_mut(), area);
}

impl App {
    pub(super) fn draw_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let [line, rule] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
            .areas(area.inner(Margin::new(2, 0)));
        // Left: who and where. Right: which project and theme the next action uses,
        // which is the context a caller most often gets wrong.
        let mut left = vec![
            Span::styled("sfumato", Style::default().fg(ACCENT).bold()),
            Span::styled("  ·  ", Style::default().fg(PANEL)),
            Span::styled(self.breadcrumb(), Style::default().fg(TEXT)),
        ];
        if self.snapshot.project.is_none() {
            left.push(Span::styled("  (no project)", Style::default().fg(RED)));
        }
        let context = match self
            .snapshot
            .project
            .as_ref()
            .and_then(|p| p.theme.as_deref())
        {
            Some(theme) => format!("{} ▸ {theme}", self.snapshot.project_name()),
            None => self.snapshot.project_name().to_string(),
        };
        let [left_area, right_area] =
            Layout::horizontal([Constraint::Min(20), Constraint::Length(40)]).areas(line);
        frame.render_widget(Paragraph::new(Line::from(left)), left_area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                compact(&context, right_area.width as usize),
                Style::default().fg(MUTED),
            )))
            .alignment(Alignment::Right),
            right_area,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "─".repeat(rule.width as usize),
                Style::default().fg(PANEL),
            ))),
            rule,
        );
    }

    pub(super) fn breadcrumb(&self) -> String {
        match self.screen {
            Screen::Home => "Workspace".to_string(),
            Screen::Browse(section) => section.title().to_string(),
            Screen::Generate => format!(
                "Generate / {}",
                match self.form.resource {
                    GenerateResource::Slides => "Slides",
                    GenerateResource::Page => "Page",
                    GenerateResource::Video => "Video",
                }
            ),
            Screen::Edit => "Edit / Slides".to_string(),
            Screen::Running => match self.resource_operation {
                ResourceOperation::Generate => "Generate / In progress".to_string(),
                ResourceOperation::GeneratePage => "Generate page / In progress".to_string(),
                ResourceOperation::GenerateVideo => "Generate video / In progress".to_string(),
                ResourceOperation::Edit => "Edit / In progress".to_string(),
            },
            Screen::Complete => match self.resource_operation {
                ResourceOperation::Generate => "Generate / Result".to_string(),
                ResourceOperation::GeneratePage => "Generate page / Result".to_string(),
                ResourceOperation::GenerateVideo => "Generate video / Result".to_string(),
                ResourceOperation::Edit => "Edit / Result".to_string(),
            },
        }
    }

    /// Keys that do something on the current screen, in the order they matter.
    ///
    /// The footer used to show only a status word, so every binding had to be
    /// guessed. Listing them is what makes a keyboard UI discoverable at all.
    pub(super) fn key_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.operation.is_some() {
            return vec![
                ("↑↓", "field"),
                ("enter", "confirm"),
                ("esc", "cancel"),
                ("?", "help"),
            ];
        }
        let mut hints: Vec<(&'static str, &'static str)> = match self.screen {
            Screen::Home => vec![("↑↓", "move"), ("enter", "open")],
            Screen::Browse(_) => vec![
                ("↑↓", "move"),
                ("←→", "actions"),
                ("enter", "run"),
                ("esc", "back"),
            ],
            Screen::Generate | Screen::Edit => vec![
                ("↑↓", "field"),
                ("space", "toggle"),
                ("enter", "start"),
                ("esc", "back"),
            ],
            Screen::Running => vec![("↑↓", "scroll"), ("esc", "cancel")],
            Screen::Complete => vec![("↑↓", "scroll"), ("enter", "home")],
        };
        hints.push(("ctrl+k", "jump"));
        hints.push(("?", "help"));
        hints.push(("q", "quit"));
        hints
    }

    pub(super) fn draw_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let [status_area, keys_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
                .areas(area.inner(Margin::new(2, 0)));

        let (message, error) = self.status.as_ref().map_or_else(
            || {
                let message = match self.screen {
                    Screen::Running => self
                        .current_stage
                        .map(|stage| format!("{}...", stage_label(stage)))
                        .unwrap_or_else(|| "working...".to_string()),
                    Screen::Complete if self.result.is_some() => self
                        .result
                        .as_ref()
                        .map(|result| result.markdown_path().display().to_string())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                (message, false)
            },
            |(message, error)| (message.clone(), *error),
        );
        if !message.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        if error { "x " } else { "> " },
                        Style::default().fg(if error { RED } else { CYAN }),
                    ),
                    Span::styled(
                        compact(&message, status_area.width.saturating_sub(2) as usize),
                        Style::default().fg(if error { RED } else { MUTED }),
                    ),
                ])),
                status_area,
            );
        }

        let mut spans = Vec::new();
        for (key, action) in self.key_hints() {
            if !spans.is_empty() {
                spans.push(Span::styled("   ", Style::default().fg(PANEL)));
            }
            spans.push(Span::styled(key, Style::default().fg(TEXT).bold()));
            spans.push(Span::styled(
                format!(" {action}"),
                Style::default().fg(MUTED),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(truncate_spans(spans, keys_area.width as usize))),
            keys_area,
        );
    }
}

pub(super) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Drops whatever a field cannot show, so a line never bleeds past its border.
pub(super) fn truncate_spans(spans: Vec<Span<'static>>, budget: usize) -> Vec<Span<'static>> {
    let mut used = 0;
    let mut kept = Vec::with_capacity(spans.len());
    for span in spans {
        let width = span.content.chars().count();
        if used + width <= budget {
            used += width;
            kept.push(span);
            continue;
        }
        let room = budget - used;
        if room > 0 {
            let content = span.content.chars().take(room).collect::<String>();
            kept.push(Span::styled(content, span.style));
        }
        break;
    }
    kept
}


pub(super) fn field_block(label: &'static str, selected: bool) -> Block<'static> {
    Block::new()
        .title(format!(" {label} "))
        .title_style(
            Style::default()
                .fg(if selected { ACCENT } else { MUTED })
                .bold(),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if selected { ACCENT } else { MUTED }))
}
