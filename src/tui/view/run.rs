//! The progress line and the activity feed of a running operation.

use super::*;

impl App {
    pub(super) fn draw_generation(&mut self, frame: &mut Frame<'_>, area: Rect) {
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
    pub(super) fn elapsed(&self) -> String {
        let seconds = self
            .started_at
            .map(|start| start.elapsed().as_secs())
            .unwrap_or(0);
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

    pub(super) fn draw_stages(&self, frame: &mut Frame<'_>, area: Rect) {
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

    pub(super) fn draw_activity(&mut self, frame: &mut Frame<'_>, area: Rect) {
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
}

pub(crate) fn stage_label(stage: GenerationStage) -> &'static str {
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
pub(super) fn activity_style(kind: ActivityKind) -> (&'static str, Color) {
    match kind {
        ActivityKind::Stage => ("▸", ACCENT),
        ActivityKind::Model => ("◆", CYAN),
        ActivityKind::ToolCall => ("⚙", MAGENTA),
        ActivityKind::ToolResult => ("↳", GREEN),
        ActivityKind::Warning => ("⚠", RED),
        ActivityKind::Success => ("✓", GREEN),
    }
}
