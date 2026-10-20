//! The Events screen's "Events by hour" heatmap: a row per weekday, a cell
//! per hour, and an hour axis whose labels sit over the cells they name.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{Datelike, Timelike};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unifly_api::Event;

use crate::tui::theme;

/// Rows the heatmap takes: the title, the hour axis, and seven weekdays.
pub(super) const PUNCHCARD_HEIGHT: u16 = 9;

/// Cells left of the first hour, holding the weekday label ("  Mon ").
const GUTTER: u16 = 6;

/// Widest an hour cell grows, so a very wide terminal still gets a grid of
/// cells rather than a row of long bars.
const MAX_HOUR_WIDTH: u16 = 8;

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

pub(super) fn render_punchcard(events: &VecDeque<Arc<Event>>, frame: &mut Frame, area: Rect) {
    let buckets = event_punchcard(events);
    let max_count = buckets
        .iter()
        .flatten()
        .copied()
        .max()
        .unwrap_or_default()
        .max(1);
    let width = hour_width(area.width);

    let mut lines = vec![
        title_line(),
        Line::from(Span::styled(
            hour_axis(width),
            Style::default().fg(theme::text_muted()),
        )),
    ];
    for (day, row) in WEEKDAYS.iter().zip(buckets.iter()) {
        let mut spans = vec![Span::styled(
            format!("  {day} "),
            Style::default().fg(theme::text_secondary()),
        )];
        for count in row {
            let (glyph, color) = punchcard_cell(*count, max_count);
            spans.push(Span::styled(
                hour_cell(glyph, width),
                Style::default().fg(color),
            ));
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn title_line() -> Line<'static> {
    let colors = theme::event_density_colors();
    let key = |glyph: &'static str, label: &'static str, color: Color| {
        [
            Span::styled(glyph, Style::default().fg(color)),
            Span::styled(label, Style::default().fg(theme::text_muted())),
        ]
    };
    let mut spans = vec![Span::styled("  Events by hour   ", theme::table_header())];
    spans.extend(key("░", " none  ", theme::text_muted()));
    spans.extend(key("▒", " some  ", colors[0]));
    spans.extend(key("▓", " busy  ", colors[1]));
    spans.extend(key("█", " peak", colors[2]));
    Line::from(spans)
}

/// Cells per hour: the room right of the gutter split 24 ways, from one
/// up to `MAX_HOUR_WIDTH`.
fn hour_width(area_width: u16) -> u16 {
    (area_width.saturating_sub(GUTTER) / 24).clamp(1, MAX_HOUR_WIDTH)
}

/// One hour's cell: its glyph across the cell, with the last column left
/// blank as a gap once cells are three or more wide.
fn hour_cell(glyph: &str, width: u16) -> String {
    let filled = if width >= 3 { width - 1 } else { width };
    let mut cell = glyph.repeat(usize::from(filled));
    cell.push_str(&" ".repeat(usize::from(width - filled)));
    cell
}

/// The hour axis: a two-digit label at the first cell of every labelled
/// hour. Labels run every hour when cells are wide enough to keep them
/// apart, else every 2, 3, or 6 hours.
fn hour_axis(width: u16) -> String {
    let step = [1, 2, 3, 6]
        .into_iter()
        .find(|step| step * width >= 4)
        .unwrap_or(6);
    let mut axis = " ".repeat(usize::from(GUTTER));
    for hour in (0..24).step_by(usize::from(step)) {
        let column = usize::from(GUTTER + hour * width);
        let padding = column.saturating_sub(axis.chars().count());
        axis.push_str(&" ".repeat(padding));
        let _ = write!(axis, "{hour:02}");
    }
    axis
}

fn event_punchcard(events: &VecDeque<Arc<Event>>) -> [[usize; 24]; 7] {
    let mut buckets = [[0usize; 24]; 7];
    for event in events {
        let day = usize::try_from(event.timestamp.weekday().num_days_from_monday()).unwrap_or(0);
        let hour = usize::try_from(event.timestamp.hour()).unwrap_or(0);
        buckets[day][hour] = buckets[day][hour].saturating_add(1);
    }
    buckets
}

fn punchcard_cell(count: usize, max_count: usize) -> (&'static str, Color) {
    if count == 0 {
        return ("░", theme::text_muted());
    }
    let colors = theme::event_density_colors();
    let max_count = max_count.max(1);
    let scaled = if max_count <= 1 || count.saturating_mul(3) <= max_count {
        1
    } else if count.saturating_mul(3) <= max_count.saturating_mul(2) {
        2
    } else {
        3
    };

    match scaled {
        1 => ("▒", colors[0]),
        2 => ("▓", colors[1]),
        _ => ("█", colors[2]),
    }
}

#[cfg(test)]
mod tests {
    use super::{GUTTER, MAX_HOUR_WIDTH, hour_axis, hour_cell, hour_width};

    #[test]
    fn hour_cells_fill_the_room_beside_the_gutter() {
        assert_eq!(hour_width(181), 7, "183-column terminal");
        assert_eq!(hour_width(98), 3, "100-column terminal");
        assert_eq!(hour_width(78), 3, "80-column terminal");
        assert_eq!(hour_width(40), 1);
        assert_eq!(hour_width(1000), MAX_HOUR_WIDTH);
    }

    #[test]
    fn every_axis_label_starts_over_its_hour_column() {
        for width in 1..=MAX_HOUR_WIDTH {
            let axis = hour_axis(width);
            let mut labelled = 0;
            for hour in 0..24u16 {
                let column = usize::from(GUTTER + hour * width);
                let label = format!("{hour:02}");
                if axis.get(column..column + 2) == Some(label.as_str()) {
                    labelled += 1;
                }
            }
            let labels = axis.split_whitespace().count();
            assert_eq!(
                labelled, labels,
                "misplaced label at width {width}: {axis:?}"
            );
            assert!(labels >= 4, "too few labels at width {width}: {axis:?}");
            let grid = usize::from(GUTTER + 24 * width);
            assert!(
                axis.chars().count() <= grid,
                "axis wider than grid: {axis:?}"
            );
        }
    }

    #[test]
    fn labels_never_touch_each_other() {
        for width in 1..=MAX_HOUR_WIDTH {
            let axis = hour_axis(width);
            assert!(
                !axis.split_whitespace().any(|label| label.len() > 2),
                "labels ran together at width {width}: {axis:?}"
            );
        }
    }

    #[test]
    fn cells_keep_a_gap_once_they_are_wide_enough() {
        assert_eq!(hour_cell("▓", 1), "▓");
        assert_eq!(hour_cell("▓", 2), "▓▓");
        assert_eq!(hour_cell("▓", 3), "▓▓ ");
        assert_eq!(hour_cell("░", 7), "░░░░░░ ");
    }
}
