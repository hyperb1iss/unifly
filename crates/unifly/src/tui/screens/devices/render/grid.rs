//! An aligned label and value grid for the detail panes, and the text
//! helpers the panes share.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;
use crate::tui::widgets::click::{self, Hint};

/// Narrowest column the grid lays fields out in.
const MIN_COLUMN: usize = 34;

/// Fields from which a wide pane splits them over two columns.
const SPLIT_FROM: usize = 8;

/// Column width from which two columns leave every value room.
const WIDE_COLUMN: usize = 48;

/// One labelled value.
#[derive(Debug, Clone)]
pub struct Field {
    label: &'static str,
    value: String,
    color: Color,
    /// A glyph drawn before the value in its own color, such as a status
    /// dot.
    marker: Option<Span<'static>>,
}

impl Field {
    pub fn new(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
            color: theme::accent_secondary(),
            marker: None,
        }
    }

    /// A field whose value may be unknown, drawn as a muted dash then.
    pub fn optional(label: &'static str, value: Option<String>) -> Self {
        match value {
            Some(value) => Self::new(label, value),
            None => Self::new(label, DASH).color(theme::text_muted()),
        }
    }

    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    #[must_use]
    pub fn marker(mut self, marker: Span<'static>) -> Self {
        self.marker = Some(marker);
        self
    }
}

/// What stands in for a value the controller did not report.
pub const DASH: &str = "\u{2500}";

/// Columns `text` takes when drawn.
pub fn width_of(text: &str) -> usize {
    Span::raw(text).width()
}

/// `text` cut to `width` columns, closing a cut with an ellipsis.
pub fn truncate(text: &str, width: usize) -> String {
    if width_of(text) <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 1;
    for ch in text.chars() {
        used += width_of(ch.encode_utf8(&mut [0; 4]));
        if used > width {
            break;
        }
        kept.push(ch);
    }
    if width > 0 {
        kept.push('\u{2026}');
    }
    kept
}

/// `text` cut to the width the table planner gave column `column`, less
/// `reserved` cells for a marker drawn after it, so a name that does not
/// fit ends in an ellipsis instead of mid-word.
pub fn clipped(widths: &[Constraint], column: usize, text: String, reserved: usize) -> String {
    match widths.get(column) {
        Some(Constraint::Length(width)) => {
            truncate(&text, usize::from(*width).saturating_sub(reserved))
        }
        _ => text,
    }
}

/// The grid's lines for an area `width` by `height`: fields run down the
/// first column and on into the next, in as few columns as hold them all,
/// with labels padded to one width so the values line up.
pub fn lines(fields: &[Field], width: u16, height: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let rows_available = usize::from(height).max(1);
    let most_columns = (width / MIN_COLUMN).max(1);
    // A long list on a wide pane reads better side by side than as one
    // column beside empty space.
    let balanced = if fields.len() >= SPLIT_FROM && width >= 2 * WIDE_COLUMN {
        2
    } else {
        1
    };
    let columns = fields
        .len()
        .div_ceil(rows_available)
        .max(balanced)
        .clamp(1, most_columns);
    let rows = fields.len().div_ceil(columns);
    let column_width = width / columns;
    let label_width = fields
        .iter()
        .map(|field| width_of(field.label))
        .max()
        .unwrap_or(0);

    (0..rows)
        .map(|row| {
            let mut spans = Vec::new();
            for column in 0..columns {
                let Some(field) = fields.get(column * rows + row) else {
                    continue;
                };
                spans.extend(cell(field, label_width, column_width));
            }
            Line::from(spans)
        })
        .collect()
}

/// One field padded to `column_width`: two cells of margin, the label, two
/// cells of gap, then the value.
fn cell(field: &Field, label_width: usize, column_width: usize) -> Vec<Span<'static>> {
    let label = format!("  {:<label_width$}  ", field.label);
    let marker_width = field.marker.as_ref().map_or(0, |marker| marker.width() + 1);
    let room = column_width.saturating_sub(width_of(&label) + marker_width + 1);
    let value = truncate(&field.value, room);
    let pad = column_width.saturating_sub(width_of(&label) + marker_width + width_of(&value));
    let mut spans = vec![Span::styled(
        label,
        Style::default().fg(theme::text_secondary()),
    )];
    if let Some(marker) = &field.marker {
        spans.push(marker.clone());
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(value, Style::default().fg(field.color)));
    spans.push(Span::raw(" ".repeat(pad)));
    spans
}

/// Screen height below which an open detail takes the whole screen: the
/// pane needs its tabs, a few lines, and its hints more than the table
/// needs two rows.
const SHARE_FROM: u16 = 12;

/// Rows the table keeps above an open detail in a screen `height` rows
/// tall: a third, between four and twelve, or none on a very short screen.
pub fn list_rows_beside_detail(height: u16) -> u16 {
    if height < SHARE_FROM {
        0
    } else {
        (height / 3).clamp(4, 12)
    }
}

/// Draw one muted line in the middle of `area`: the empty, loading, and
/// no-match states of a list.
pub fn note(frame: &mut Frame, area: Rect, text: &str) {
    if area.is_empty() {
        return;
    }
    let row = Rect {
        y: area.y + area.height.saturating_sub(1) / 2,
        height: 1,
        ..area
    };
    let text = truncate(text, usize::from(area.width));
    frame.render_widget(
        Paragraph::new(Span::styled(text, Style::default().fg(theme::text_muted())))
            .alignment(Alignment::Center),
        row,
    );
}

/// The hints that fit `width`, in the order given. Each carries a rank,
/// and the highest ranks are left out first when the bar is too narrow, so
/// a hint bar never runs off the edge mid-word.
pub fn fit_hints(mut hints: Vec<(u8, Hint)>, width: u16) -> Vec<Hint> {
    let bar = |hints: &[(u8, Hint)]| {
        let shown: Vec<Hint> = hints.iter().map(|(_, hint)| hint.clone()).collect();
        click::key_hints(&shown).width()
    };
    while hints.len() > 1 && bar(&hints) > usize::from(width) {
        let drop = hints
            .iter()
            .enumerate()
            .max_by_key(|(_, (rank, _))| *rank)
            .map_or(0, |(index, _)| index);
        hints.remove(drop);
    }
    hints.into_iter().map(|(_, hint)| hint).collect()
}

/// A timestamp in the terminal's own time zone.
pub fn local_time(time: chrono::DateTime<chrono::Utc>) -> String {
    time.with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// How long ago `time` was, such as `4h 23m ago`.
pub fn ago(time: chrono::DateTime<chrono::Utc>) -> String {
    let secs = chrono::Utc::now()
        .signed_duration_since(time)
        .num_seconds()
        .max(0);
    let secs = u64::try_from(secs).unwrap_or_default();
    if secs < 60 {
        return "just now".into();
    }
    format!("{} ago", crate::tui::widgets::bytes_fmt::fmt_uptime(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn values_line_up_under_each_other_whatever_the_label_length() {
        let fields = [
            Field::new("IP", "10.40.0.2"),
            Field::new("Firmware", "7.2.123"),
            Field::new("Uplink", "gw-harbor port 9"),
        ];
        let rows = text(&lines(&fields, 40, 10));
        assert_eq!(rows.len(), 3);
        let starts: Vec<_> = ["10.40.0.2", "7.2.123", "gw-harbor"]
            .iter()
            .zip(&rows)
            .map(|(value, row)| row.find(value))
            .collect();
        assert!(starts.iter().all(|start| *start == starts[0]), "{rows:?}");
    }

    #[test]
    fn a_short_area_flows_the_fields_into_a_second_column() {
        let fields: Vec<_> = ["A", "B", "C", "D", "E"]
            .into_iter()
            .map(|label| Field::new(label, "value"))
            .collect();
        let rows = text(&lines(&fields, 80, 3));
        assert_eq!(rows.len(), 3);
        assert!(rows[0].contains('A') && rows[0].contains('D'), "{rows:?}");
        // Too narrow for two columns: every field still gets a line.
        assert_eq!(lines(&fields, 40, 3).len(), 5);
    }

    #[test]
    fn a_cut_value_ends_in_an_ellipsis_and_fits_its_column() {
        assert_eq!(truncate("dailies-review-station", 10), "dailies-r\u{2026}");
        assert_eq!(truncate("short", 10), "short");
        // Wide glyphs count by the cells they take.
        assert!(width_of(&truncate("渲染渲染渲染渲染", 7)) <= 7);
    }
}
