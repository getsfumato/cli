//! Form rendering: one row per field, and the controls a row can hold.

use super::*;

impl App {
    pub(super) fn draw_generate(&mut self, frame: &mut Frame<'_>, area: Rect) {
        draw_resource_form(frame, area, &self.form.fields, self.form.selected);
    }

    pub(super) fn draw_edit(&mut self, frame: &mut Frame<'_>, area: Rect) {
        draw_resource_form(frame, area, &self.edit_form.fields, self.edit_form.selected);
    }
}

/// Renders a form as one row per field.
///
/// Each field used to be its own bordered box: three rows of chrome around one line
/// of content, so the video form needed about sixty rows in a terminal that has
/// twenty-four. The label now sits in a fixed left column and the value beside it,
/// with focus shown by a marker and a highlight instead of a border — which also
/// makes the focused field easier to find than a box among boxes.
pub(crate) fn draw_resource_form(
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
                    Span::styled(*placeholder, Style::default().fg(FAINT))
                } else {
                    Span::styled(
                        compact(value, value_width as usize),
                        Style::default().fg(TEXT),
                    )
                };
                let _ = multiline;
                line.push(shown);
            }
            // Shows the chosen identifier, or what will be used when nothing is
            // chosen — the same phrasing the free-text placeholder used, so the
            // default is still stated rather than implied by an empty field.
            FormField::Choice {
                value, placeholder, ..
            } => {
                // A caret, because a picker that looks exactly like a text field
                // teaches the user to type an identifier they would have to know.
                line.push(Span::styled(
                    "▾ ",
                    Style::default().fg(if selected { ACCENT } else { FAINT }),
                ));
                line.push(if value.is_empty() {
                    Span::styled(*placeholder, Style::default().fg(FAINT))
                } else {
                    Span::styled(value.clone(), Style::default().fg(TEXT))
                });
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

/// Renders the option row, windowed so the `[x]` marker is always on screen.
///
/// The row is a single unwrapped, unscrolled line inside a bordered field, so a
/// full option list overflows an 80-column terminal and can clip the selection
/// out of sight while `OperationForm::select` still returns it. `width` is the
/// field's inner width; markers show which side was truncated.
pub(crate) fn select_line(options: &[String], selected: usize, width: u16) -> Line<'static> {
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

pub(crate) fn multi_select_line(
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
pub(crate) fn field_height(_field: &FormField) -> u16 {
    1
}

pub(crate) fn visible_field_range(
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
