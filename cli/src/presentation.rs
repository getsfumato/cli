//! Human-readable terminal presentation helpers.

use std::{env, io::IsTerminal};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tone {
    #[default]
    Plain,
    Primary,
    Success,
    Warning,
    Muted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cell {
    text: String,
    tone: Tone,
}

impl Cell {
    pub(crate) fn new(value: impl ToString) -> Self {
        Self {
            text: value.to_string(),
            tone: Tone::Plain,
        }
    }

    pub(crate) fn primary(value: impl ToString) -> Self {
        Self {
            text: value.to_string(),
            tone: Tone::Primary,
        }
    }

    pub(crate) fn success(value: impl ToString) -> Self {
        Self {
            text: value.to_string(),
            tone: Tone::Success,
        }
    }

    pub(crate) fn warning(value: impl ToString) -> Self {
        Self {
            text: value.to_string(),
            tone: Tone::Warning,
        }
    }

    pub(crate) fn muted(value: impl ToString) -> Self {
        Self {
            text: value.to_string(),
            tone: Tone::Muted,
        }
    }
}

pub(crate) fn print_table(headers: &[&str], rows: Vec<Vec<Cell>>) {
    println!("{}", render_table(headers, &rows, colors_enabled()));
}

fn colors_enabled() -> bool {
    std::io::stdout().is_terminal()
        && env::var_os("NO_COLOR").is_none()
        && env::var("TERM").is_ok_and(|term| term != "dumb")
}

fn render_table(headers: &[&str], rows: &[Vec<Cell>], colors: bool) -> String {
    if headers.is_empty() {
        return String::new();
    }
    let widths = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter()
                .filter_map(|row| row.get(index))
                .map(|cell| display_width(&cell.text))
                .fold(display_width(header), usize::max)
        })
        .collect::<Vec<_>>();
    let border = border(&widths);
    let mut lines = vec![border.clone()];
    lines.push(render_row(
        &headers
            .iter()
            .map(|header| Cell::primary(*header))
            .collect::<Vec<_>>(),
        &widths,
        colors,
    ));
    lines.push(border.clone());
    lines.extend(rows.iter().map(|row| render_row(row, &widths, colors)));
    lines.push(border);
    lines.join("\n")
}

fn border(widths: &[usize]) -> String {
    format!(
        "+{}+",
        widths
            .iter()
            .map(|width| "-".repeat(width + 2))
            .collect::<Vec<_>>()
            .join("+")
    )
}

fn render_row(cells: &[Cell], widths: &[usize], colors: bool) -> String {
    let columns = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            let cell = cells.get(index).cloned().unwrap_or_default();
            let padding = " ".repeat(width.saturating_sub(display_width(&cell.text)));
            format!(" {}{} ", paint(&cell.text, cell.tone, colors), padding)
        })
        .collect::<Vec<_>>()
        .join("|");
    format!("|{columns}|")
}

fn paint(value: &str, tone: Tone, enabled: bool) -> String {
    if !enabled || tone == Tone::Plain {
        return value.to_string();
    }
    let code = match tone {
        Tone::Plain => return value.to_string(),
        Tone::Primary => "1;36",
        Tone::Success => "1;32",
        Tone::Warning => "1;33",
        Tone::Muted => "2;37",
    };
    format!("\u{1b}[{code}m{value}\u{1b}[0m")
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

#[cfg(test)]
// Test bodies live under tests/unit so presentation code remains focused.
#[path = "../tests/unit/presentation.rs"]
mod tests;
