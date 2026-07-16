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
