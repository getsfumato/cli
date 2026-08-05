//! Ratatui rendering entry point.

use super::*;

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
    app.draw_footer(frame, footer);
    app.effects
        .process_effects(TICK_RATE.into(), frame.buffer_mut(), area);
}

impl App {
    fn draw_header(&self, frame: &mut Frame<'_>, area: Rect) {
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

    fn breadcrumb(&self) -> String {
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

    fn draw_home(&mut self, frame: &mut Frame<'_>, area: Rect) {
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

    fn draw_browse(&mut self, frame: &mut Frame<'_>, area: Rect, section: Section) {
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

    fn draw_operation(&self, frame: &mut Frame<'_>, area: Rect) {
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

    fn draw_generate(&mut self, frame: &mut Frame<'_>, area: Rect) {
        draw_resource_form(frame, area, &self.form.fields, self.form.selected);
    }

    fn draw_edit(&mut self, frame: &mut Frame<'_>, area: Rect) {
        draw_resource_form(frame, area, &self.edit_form.fields, self.edit_form.selected);
    }

    fn draw_generation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let inner = area.inner(Margin::new(2, 0));
        // Two rows of progress, then the feed. The pipeline used to be a
        // twenty-four-column box down the full height, so seven stage names occupied
        // a quarter of the width and most of it was empty — while the activity feed,
        // which is the part that changes, was squeezed beside it.
        let [progress_area, feed_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(inner);
        self.draw_stages(frame, progress_area);

        let has_image = self.image.is_some() && feed_area.width >= 70;
        let [activity_area, preview_area] = if has_image {
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(feed_area)
        } else {
            [feed_area, Rect::default()]
        };
        self.draw_activity(frame, activity_area);
        if has_image {
            let preview = Block::new()
                .title(" PREVIEW ")
                .title_style(Style::default().fg(MAGENTA).bold())
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(PANEL));
            let content = preview.inner(preview_area).inner(Margin::new(1, 0));
            frame.render_widget(preview, preview_area);
            if let Some(image) = &mut self.image {
                StatefulImage::default().resize(Resize::Fit(None)).render(
                    content,
                    frame.buffer_mut(),
                    image,
                );
            }
        }
    }

    /// Formats the elapsed run time as `m:ss`.
    fn elapsed(&self) -> String {
        let seconds = self
            .started_at
            .map(|start| start.elapsed().as_secs())
            .unwrap_or(0);
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

    fn draw_stages(&self, frame: &mut Frame<'_>, area: Rect) {
        let generation_stages = [
            GenerationStage::Draft,
            GenerationStage::ValidationRepair,
            GenerationStage::SemanticReview,
            GenerationStage::DiagramRepair,
            GenerationStage::LayoutCheck,
            GenerationStage::LayoutRepair,
            GenerationStage::Rendering,
        ];
        let edit_stages = [
            GenerationStage::Edit,
            GenerationStage::LayoutCheck,
            GenerationStage::Rendering,
        ];
        let page_stages = [
            GenerationStage::PageDraft,
            GenerationStage::PageReview,
            GenerationStage::LayoutCheck,
            GenerationStage::PageRepair,
            GenerationStage::PageRendering,
        ];
        let video_stages = [
            GenerationStage::VideoPlanning,
            GenerationStage::VideoReview,
            GenerationStage::VideoAuthoring,
            GenerationStage::VideoRepair,
            GenerationStage::VideoRendering,
        ];
        let stages: &[GenerationStage] = match self.resource_operation {
            ResourceOperation::Generate => &generation_stages,
            ResourceOperation::GeneratePage => &page_stages,
            ResourceOperation::GenerateVideo => &video_stages,
            ResourceOperation::Edit => &edit_stages,
        };
        let current = self
            .current_stage
            .and_then(|current| stages.iter().position(|stage| *stage == current))
            .unwrap_or(0);
        let running = self.screen == Screen::Running;
        let completed = self.screen == Screen::Complete && !self.generation_failed;
        // Braille spinner rather than `|/-\\`: it reads as motion instead of as
        // punctuation, which matters when it is the only thing moving on screen.
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][self.tick % 10];

        let (marker, marker_colour) = if self.generation_failed {
            ("✗", RED)
        } else if completed {
            ("✓", GREEN)
        } else {
            (spinner, ACCENT)
        };
        let stage_name = stages
            .get(current)
            .map(|stage| stage_label(*stage))
            .unwrap_or("Working");
        let [headline, dots, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        let counter = format!("step {}/{}", current + 1, stages.len());
        let elapsed = self.elapsed();
        let right = format!("{counter}   {elapsed}");
        let [name_area, right_area] = Layout::horizontal([
            Constraint::Min(10),
            Constraint::Length(right.len() as u16 + 2),
        ])
        .areas(headline);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("{marker} "),
                    Style::default().fg(marker_colour).bold(),
                ),
                Span::styled(stage_name, Style::default().fg(TEXT).bold()),
            ])),
            name_area,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(right, Style::default().fg(MUTED))))
                .alignment(Alignment::Right),
            right_area,
        );

        // One glyph per stage: the whole pipeline at a glance, in one row, instead of
        // one labelled row per stage down a column.
        let mut spans = Vec::new();
        for (index, stage) in stages.iter().enumerate() {
            let (glyph, colour) = if index < current || completed {
                ("●", GREEN)
            } else if index == current && self.generation_failed {
                ("●", RED)
            } else if index == current && running {
                ("◐", ACCENT)
            } else {
                ("○", PANEL)
            };
            spans.push(Span::styled(glyph, Style::default().fg(colour)));
            spans.push(Span::raw(" "));
            let _ = stage;
        }
        frame.render_widget(
            Paragraph::new(Line::from(truncate_spans(spans, dots.width as usize))),
            dots,
        );
    }

    fn draw_activity(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let items = self
            .activities
            .iter()
            .map(|activity| {
                let (marker, color) = activity_style(activity.kind);
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{marker} "), Style::default().fg(color).bold()),
                        Span::styled(&activity.title, Style::default().fg(TEXT).bold()),
                    ]),
                    Line::from(Span::styled(
                        format!("  {}", compact(&activity.detail, 180)),
                        Style::default().fg(MUTED),
                    )),
                ])
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            let waiting = if self.screen == Screen::Running {
                self.current_stage
                    .map(|stage| format!("{}...", stage_label(stage)))
                    .unwrap_or_else(|| "starting...".to_string())
            } else {
                "nothing was recorded".to_string()
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        if self.screen == Screen::Running {
                            "ACTIVITY"
                        } else {
                            "RESULT"
                        },
                        Style::default().fg(CYAN).bold(),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(waiting, Style::default().fg(MUTED))),
                ]),
                area,
            );
            return;
        }
        let mut state = ListState::default()
            .with_selected((!self.activities.is_empty()).then_some(self.activity_index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(PANEL))
                .block(
                    Block::new()
                        .title(if self.screen == Screen::Running {
                            " ACTIVITY "
                        } else {
                            " RESULT "
                        })
                        .title_style(Style::default().fg(CYAN).bold())
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(PANEL)),
                ),
            area,
            &mut state,
        );
        if self.activities.len() > area.height.saturating_sub(2) as usize {
            let mut scrollbar =
                ScrollbarState::new(self.activities.len()).position(self.activity_index);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight),
                area.inner(Margin::new(0, 1)),
                &mut scrollbar,
            );
        }
    }

    /// Keys that do something on the current screen, in the order they matter.
    ///
    /// The footer used to show only a status word, so every binding had to be
    /// guessed. Listing them is what makes a keyboard UI discoverable at all.
    fn key_hints(&self) -> Vec<(&'static str, &'static str)> {
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
        hints.push(("?", "help"));
        hints.push(("q", "quit"));
        hints
    }

    fn draw_footer(&self, frame: &mut Frame<'_>, area: Rect) {
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

/// Renders a form as one row per field.
///
/// Each field used to be its own bordered box: three rows of chrome around one line
/// of content, so the video form needed about sixty rows in a terminal that has
/// twenty-four. The label now sits in a fixed left column and the value beside it,
/// with focus shown by a marker and a highlight instead of a border — which also
/// makes the focused field easier to find than a box among boxes.
fn draw_resource_form(
    frame: &mut Frame<'_>,
    area: Rect,
    fields: &[FormField],
    selected_index: usize,
) {
    /// Width of the label column, sized to the longest label the forms use.
    const LABEL_WIDTH: usize = 18;
    let form_area = area.inner(Margin::new(2, 0));
    let range = visible_field_range(fields, selected_index, form_area.height);
    let visible_fields = &fields[range.clone()];
    let rows = Layout::vertical(
        visible_fields
            .iter()
            .map(|field| Constraint::Length(field_height(field)))
            .collect::<Vec<_>>(),
    )
    .split(form_area);
    let value_width = form_area.width.saturating_sub(LABEL_WIDTH as u16 + 4);

    for (offset, (field, row)) in visible_fields.iter().zip(rows.iter()).enumerate() {
        let index = range.start + offset;
        let selected = index == selected_index;

        if let FormField::Submit { .. } = field {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  {}  ", field.label()),
                    Style::default()
                        .fg(if selected { BG } else { TEXT })
                        .bg(if selected { ACCENT } else { PANEL })
                        .bold(),
                ))),
                *row,
            );
            continue;
        }

        let marker = Span::styled(
            if selected { "› " } else { "  " },
            Style::default().fg(ACCENT).bold(),
        );
        // Truncated to leave a gap, so a label as long as the column cannot run
        // into its own value.
        let label = Span::styled(
            format!("{:<LABEL_WIDTH$}", compact(field.label(), LABEL_WIDTH - 1)),
            if selected {
                Style::default().fg(TEXT).bold()
            } else {
                Style::default().fg(MUTED)
            },
        );
        let mut line = vec![marker, label];

        match field {
            FormField::Text {
                value,
                placeholder,
                multiline,
                ..
            } => {
                let shown = if value.is_empty() {
                    Span::styled(*placeholder, Style::default().fg(PANEL))
                } else {
                    Span::styled(
                        compact(value, value_width as usize),
                        Style::default().fg(TEXT),
                    )
                };
                let _ = multiline;
                line.push(shown);
            }
            FormField::Toggle { value, .. } => line.push(Span::styled(
                if *value { "on" } else { "off" },
                Style::default().fg(if *value { GREEN } else { MUTED }),
            )),
            FormField::Select {
                options,
                selected: choice,
                ..
            } => line.extend(select_line(options, *choice, value_width).spans),
            FormField::MultiSelect {
                options,
                cursor,
                selected: choices,
                ..
            } => line.extend(multi_select_line(options, *cursor, choices).spans),
            FormField::Submit { .. } => unreachable!("handled above"),
        }

        let style = if selected {
            Style::default().bg(PANEL)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(Line::from(truncate_spans(line, row.width as usize))).style(style),
            *row,
        );
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Renders the option row, windowed so the `[x]` marker is always on screen.
///
/// The row is a single unwrapped, unscrolled line inside a bordered field, so a
/// full option list overflows an 80-column terminal and can clip the selection
/// out of sight while `OperationForm::select` still returns it. `width` is the
/// field's inner width; markers show which side was truncated.
pub(super) fn select_line(options: &[String], selected: usize, width: u16) -> Line<'static> {
    const MARKER: &str = "<";
    let entry = |index: usize| {
        format!(
            "[{}] {}  ",
            if index == selected { "x" } else { " " },
            options[index]
        )
    };
    let budget = usize::from(width);
    if options.is_empty() || budget == 0 {
        return Line::from(Span::raw(""));
    }
    let mut first = selected.min(options.len() - 1);
    let mut last = first;
    let mut used = entry(first).chars().count();
    // Grow forwards first so the list reads left to right from the selection.
    loop {
        let mut grew = false;
        if last + 1 < options.len() {
            let width = entry(last + 1).chars().count() + MARKER.len();
            if used + width <= budget {
                used += entry(last + 1).chars().count();
                last += 1;
                grew = true;
            }
        }
        if first > 0 {
            let width = entry(first - 1).chars().count() + MARKER.len();
            if used + width <= budget {
                used += entry(first - 1).chars().count();
                first -= 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut spans = Vec::new();
    // Truncation markers are added only when they fit: on a field too narrow for
    // even the selected option, showing the choice wins over showing the arrows.
    if first > 0 && used + MARKER.len() <= budget {
        used += MARKER.len();
        spans.push(Span::styled(MARKER, Style::default().fg(MUTED)));
    }
    for (index, option) in options.iter().enumerate().take(last + 1).skip(first) {
        let active = index == selected;
        spans.push(Span::styled(
            if active { "[x] " } else { "[ ] " },
            Style::default().fg(if active { GREEN } else { MUTED }),
        ));
        spans.push(Span::styled(option.clone(), Style::default().fg(TEXT)));
        spans.push(Span::raw("  "));
    }
    if last + 1 < options.len() && used < budget {
        spans.push(Span::styled(">", Style::default().fg(MUTED)));
    }
    Line::from(truncate_spans(spans, budget))
}

/// Drops whatever a field cannot show, so a line never bleeds past its border.
fn truncate_spans(spans: Vec<Span<'static>>, budget: usize) -> Vec<Span<'static>> {
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

fn multi_select_line(
    options: &[String],
    cursor: usize,
    selected: &BTreeSet<usize>,
) -> Line<'static> {
    if options.is_empty() {
        return Line::from(Span::styled(
            "No installed plugins",
            Style::default().fg(MUTED),
        ));
    }
    Line::from(
        options
            .iter()
            .enumerate()
            .flat_map(|(index, option)| {
                let enabled = selected.contains(&index);
                vec![
                    Span::styled(
                        if index == cursor { ">" } else { " " },
                        Style::default().fg(ACCENT),
                    ),
                    Span::styled(
                        if enabled { "[x] " } else { "[ ] " },
                        Style::default().fg(if enabled { GREEN } else { MUTED }),
                    ),
                    Span::styled(option.clone(), Style::default().fg(TEXT)),
                    Span::raw("  "),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

/// Rows one field occupies.
///
/// One, where it used to be three: the border is gone, so a field costs exactly the
/// line it displays. A multiline text field keeps a second row for its overflow.
/// Rows one field occupies.
///
/// One, where it used to be three: the border is gone, so a field costs exactly the
/// line it displays. A multiline field is no taller — reserving a second row left a
/// blank line under it far more often than it showed anything, and the value is held
/// in full by the field regardless of how much of it fits on screen.
fn field_height(_field: &FormField) -> u16 {
    1
}

pub(super) fn visible_field_range(
    fields: &[FormField],
    selected: usize,
    available_height: u16,
) -> std::ops::Range<usize> {
    if fields.is_empty() || available_height == 0 {
        return 0..0;
    }
    let selected = selected.min(fields.len() - 1);
    let mut start = selected;
    let mut used = field_height(&fields[selected]);
    while start > 0 {
        let previous = field_height(&fields[start - 1]);
        if used.saturating_add(previous) > available_height {
            break;
        }
        start -= 1;
        used += previous;
    }
    let mut end = selected + 1;
    while end < fields.len() {
        let next = field_height(&fields[end]);
        if used.saturating_add(next) > available_height {
            break;
        }
        used += next;
        end += 1;
    }
    start..end
}

fn panel(title: &'static str) -> Block<'static> {
    Block::new()
        .title(format!(" {title} "))
        .title_style(Style::default().fg(CYAN).bold())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .style(Style::default().bg(BG))
}

fn field_block(label: &'static str, selected: bool) -> Block<'static> {
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

pub(super) fn stage_label(stage: GenerationStage) -> &'static str {
    match stage {
        GenerationStage::Draft => "Draft",
        GenerationStage::Edit => "Content edit",
        GenerationStage::ValidationRepair => "Structure repair",
        GenerationStage::SemanticReview => "Content review",
        GenerationStage::DiagramRepair => "Diagram repair",
        GenerationStage::LayoutCheck => "Layout check",
        GenerationStage::LayoutRepair => "Layout repair",
        GenerationStage::Rendering => "Rendering",
        GenerationStage::PageDraft => "Page draft",
        GenerationStage::PageReview => "Page review",
        GenerationStage::PageRepair => "Page repair",
        GenerationStage::PageRendering => "Page rendering",
        GenerationStage::VideoPlanning => "Video plan",
        GenerationStage::VideoReview => "Video review",
        GenerationStage::VideoAuthoring => "Video authoring",
        GenerationStage::VideoRepair => "Video repair",
        GenerationStage::VideoVisualReview => "Video frame review",
        GenerationStage::VideoRendering => "Video rendering",
        GenerationStage::VideoNarration => "Video narration",
        GenerationStage::DocumentDraft => "Document draft",
        GenerationStage::DocumentValidationRepair => "Document structure repair",
        GenerationStage::DocumentDiagramRepair => "Document diagram repair",
        GenerationStage::DocumentReview => "Document review",
        GenerationStage::DocumentFormatCheck => "Page format check",
        GenerationStage::DocumentFormatRepair => "Page format repair",
        GenerationStage::DocumentRendering => "Document rendering",
    }
}

/// Marker and colour for one feed entry.
///
/// Distinct glyphs per kind: `Stage` and `ToolCall` both used `>` and `ToolResult`
/// and `Success` both used `+`, so the feed could not be skimmed by shape — only by
/// colour, which is exactly what is lost when the output is piped or the user cannot
/// distinguish the hues.
fn activity_style(kind: ActivityKind) -> (&'static str, Color) {
    match kind {
        ActivityKind::Stage => ("▸", ACCENT),
        ActivityKind::Model => ("◆", CYAN),
        ActivityKind::ToolCall => ("⚙", MAGENTA),
        ActivityKind::ToolResult => ("↳", GREEN),
        ActivityKind::Warning => ("⚠", RED),
        ActivityKind::Success => ("✓", GREEN),
    }
}
