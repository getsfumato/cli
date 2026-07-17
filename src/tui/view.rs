//! Ratatui rendering entry point.

use super::*;

pub(super) fn draw(app: &mut App, frame: &mut Frame<'_>) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::default().bg(BG).fg(TEXT)), area);
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(if area.height >= 24 { 6 } else { 3 }),
        Constraint::Min(8),
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
        if area.height >= 6 && area.width >= 100 {
            let [brand, context] =
                Layout::horizontal([Constraint::Length(60), Constraint::Min(10)])
                    .areas(area.inner(Margin::new(2, 0)));
            let logo = BigText::builder()
                .pixel_size(PixelSize::HalfHeight)
                .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
                .lines(vec![Line::from("SFUMATO")])
                .build();
            frame.render_widget(logo, brand);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "STUDY RESOURCE ENGINE",
                        Style::default().fg(CYAN),
                    )),
                    Line::from(Span::styled(self.breadcrumb(), Style::default().fg(MUTED))),
                ])
                .alignment(Alignment::Right),
                context,
            );
        } else {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" SFUMATO ", Style::default().fg(BG).bg(ACCENT).bold()),
                    Span::raw("  "),
                    Span::styled(self.breadcrumb(), Style::default().fg(MUTED)),
                ])),
                area,
            );
        }
    }

    fn breadcrumb(&self) -> String {
        match self.screen {
            Screen::Home => "Workspace".to_string(),
            Screen::Browse(section) => section.title().to_string(),
            Screen::Generate => format!(
                "Generate / {}",
                if self.form.is_page() {
                    "Page"
                } else {
                    "Slides"
                }
            ),
            Screen::Edit => "Edit / Slides".to_string(),
            Screen::Running => match self.resource_operation {
                ResourceOperation::Generate => "Generate / In progress".to_string(),
                ResourceOperation::GeneratePage => "Generate page / In progress".to_string(),
                ResourceOperation::Edit => "Edit / In progress".to_string(),
            },
            Screen::Complete => match self.resource_operation {
                ResourceOperation::Generate => "Generate / Result".to_string(),
                ResourceOperation::GeneratePage => "Generate page / Result".to_string(),
                ResourceOperation::Edit => "Edit / Result".to_string(),
            },
        }
    }

    fn draw_home(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let [menu_area, context_area] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(area.inner(Margin::new(2, 0)));
        let items = NAV_ITEMS
            .iter()
            .enumerate()
            .map(|(index, (title, subtitle))| {
                let marker = if index == self.nav_index { ">" } else { " " };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{marker} "), Style::default().fg(ACCENT)),
                        Span::styled(*title, Style::default().fg(TEXT).bold()),
                    ]),
                    Line::from(Span::styled(
                        format!("   {subtitle}"),
                        Style::default().fg(MUTED),
                    )),
                ])
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default().with_selected(Some(self.nav_index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(PANEL))
                .block(panel("WORKSPACE")),
            menu_area,
            &mut state,
        );

        let project = self
            .application
            .list_projects()
            .ok()
            .and_then(|projects| projects.into_iter().find(|project| project.active));
        let project_name = project
            .as_ref()
            .map(|project| project.name.as_str())
            .unwrap_or("No active project");
        let project_path = project
            .as_ref()
            .map(|project| project.path.display().to_string())
            .unwrap_or_else(|| "Create or activate a project".to_string());
        let models = self
            .application
            .list_models()
            .map(|models| models.len())
            .unwrap_or(0);
        let connectors = self
            .application
            .list_connectors()
            .map(|connectors| connectors.len())
            .unwrap_or(0);
        let themes = self
            .application
            .list_themes()
            .map(|themes| themes.len())
            .unwrap_or(0);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "ACTIVE PROJECT",
                    Style::default().fg(CYAN).bold(),
                )),
                Line::from(Span::styled(project_name, Style::default().fg(TEXT).bold())),
                Line::from(Span::styled(project_path, Style::default().fg(MUTED))),
                Line::from(""),
                metric_line("Model profiles", models),
                metric_line("Connectors", connectors),
                metric_line("Themes", themes),
            ])
            .wrap(Wrap { trim: true })
            .block(panel("CONTEXT")),
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
                        Paragraph::new(select_line(options, *choice))
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
        let [stages_area, main_area] =
            Layout::horizontal([Constraint::Length(24), Constraint::Min(30)]).areas(inner);
        self.draw_stages(frame, stages_area);
        let has_image = self.image.is_some() && main_area.width >= 70;
        let [activity_area, preview_area] = if has_image {
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .areas(main_area)
        } else {
            [main_area, Rect::default()]
        };
        self.draw_activity(frame, activity_area);
        if has_image {
            let preview = Block::new()
                .title(" IMAGE PREVIEW ")
                .title_style(Style::default().fg(MAGENTA).bold())
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(MUTED));
            let content = preview.inner(preview_area).inner(Margin::new(1, 1));
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
        let stages: &[GenerationStage] = match self.resource_operation {
            ResourceOperation::Generate => &generation_stages,
            ResourceOperation::GeneratePage => &page_stages,
            ResourceOperation::Edit => &edit_stages,
        };
        let current = self
            .current_stage
            .and_then(|current| stages.iter().position(|stage| *stage == current))
            .unwrap_or(0);
        let running = self.screen == Screen::Running;
        let completed = self.screen == Screen::Complete && !self.generation_failed;
        let spinner = ["|", "/", "-", "\\"][self.tick % 4];
        let lines = stages
            .iter()
            .enumerate()
            .map(|(index, stage)| {
                let (marker, color) = if index < current || completed {
                    ("+", GREEN)
                } else if index == current && running {
                    (spinner, ACCENT)
                } else if index == current && self.generation_failed {
                    ("!", RED)
                } else {
                    (".", MUTED)
                };
                Line::from(vec![
                    Span::styled(format!(" {marker} "), Style::default().fg(color).bold()),
                    Span::styled(
                        stage_label(*stage),
                        Style::default().fg(if index <= current { TEXT } else { MUTED }),
                    ),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).block(panel("PIPELINE")), area);
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
        let mut state = ListState::default()
            .with_selected((!self.activities.is_empty()).then_some(self.activity_index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(PANEL))
                .block(panel(if self.screen == Screen::Running {
                    "ACTIVITY"
                } else {
                    "RESULT"
                })),
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

    fn draw_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let (message, error) = self.status.as_ref().map_or_else(
            || {
                let message = match self.screen {
                    Screen::Running => "Resource operation is running".to_string(),
                    Screen::Complete if self.result.is_some() => self
                        .result
                        .as_ref()
                        .map(|result| format!("Artifact: {}", result.markdown_path().display()))
                        .unwrap_or_default(),
                    _ => "Ready".to_string(),
                };
                (message, false)
            },
            |(message, error)| (message.clone(), *error),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", Style::default().bg(if error { RED } else { CYAN })),
                Span::raw(" "),
                Span::styled(
                    compact(&message, area.width.saturating_sub(4) as usize),
                    Style::default().fg(if error { RED } else { MUTED }),
                ),
            ])),
            area,
        );
    }
}

fn draw_resource_form(
    frame: &mut Frame<'_>,
    area: Rect,
    fields: &[FormField],
    selected_index: usize,
) {
    let form_area = area.inner(Margin::new(3, 0));
    let range = visible_field_range(fields, selected_index, form_area.height);
    let visible_fields = &fields[range.clone()];
    let rows = Layout::vertical(
        visible_fields
            .iter()
            .map(|field| Constraint::Length(field_height(field)))
            .collect::<Vec<_>>(),
    )
    .split(form_area);
    for (offset, (field, row)) in visible_fields.iter().zip(rows.iter()).enumerate() {
        let index = range.start + offset;
        let selected = index == selected_index;
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
                    Paragraph::new(text)
                        .wrap(Wrap { trim: false })
                        .block(field_block(label, selected)),
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
                    Paragraph::new(select_line(options, *choice))
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
            FormField::Submit { .. } => {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        field.label(),
                        Style::default().fg(if selected { BG } else { TEXT }).bold(),
                    )))
                    .alignment(Alignment::Center)
                    .style(if selected {
                        Style::default().bg(ACCENT)
                    } else {
                        Style::default().bg(PANEL)
                    })
                    .block(
                        Block::new()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded),
                    ),
                    *row,
                );
            }
        }
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

fn select_line(options: &[String], selected: usize) -> Line<'static> {
    Line::from(
        options
            .iter()
            .enumerate()
            .flat_map(|(index, option)| {
                let active = index == selected;
                vec![
                    Span::styled(
                        if active { "[x] " } else { "[ ] " },
                        Style::default().fg(if active { GREEN } else { MUTED }),
                    ),
                    Span::styled(option.clone(), Style::default().fg(TEXT)),
                    Span::raw("  "),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

fn multi_select_line(
    options: &[String],
    cursor: usize,
    selected: &BTreeSet<usize>,
) -> Line<'static> {
    if options.is_empty() {
        return Line::from(Span::styled(
            "No bundled plugins",
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

fn field_height(field: &FormField) -> u16 {
    match field {
        FormField::Text {
            multiline: true, ..
        } => 4,
        _ => 3,
    }
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

fn metric_line(label: &str, value: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{value:>3}"), Style::default().fg(ACCENT).bold()),
        Span::styled(format!("  {label}"), Style::default().fg(MUTED)),
    ])
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
    }
}

fn activity_style(kind: ActivityKind) -> (&'static str, Color) {
    match kind {
        ActivityKind::Stage => (">", ACCENT),
        ActivityKind::Model => ("~", CYAN),
        ActivityKind::ToolCall => (">", MAGENTA),
        ActivityKind::ToolResult => ("+", GREEN),
        ActivityKind::Warning => ("!", RED),
        ActivityKind::Success => ("+", GREEN),
    }
}
