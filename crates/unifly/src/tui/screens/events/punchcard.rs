//! The Events screen's heatmap of events by hour: a row per weekday, a
//! cell per hour, and an hour axis whose labels sit over the cells they
//! name, with the density scale at its right end.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{Datelike, Local, TimeZone, Timelike};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unifly_api::Event;

use crate::tui::theme;

/// Rows the heatmap takes: the hour axis and seven weekdays.
pub(super) const PUNCHCARD_HEIGHT: u16 = 8;

/// Cells left of the first hour, holding the weekday label ("  Mon ").
const GUTTER: u16 = 6;

/// Widest an hour cell grows, so a very wide terminal still gets a grid of
/// cells rather than a row of long bars.
const MAX_HOUR_WIDTH: u16 = 8;

/// Narrowest an hour cell may be for the legend to take room from the
/// grid: at three columns a cell still keeps its gap.
const MIN_HOUR_WIDTH_BESIDE_LEGEND: u16 = 3;

/// The density scale, lightest to heaviest.
const LEGEND: [&str; 3] = ["less ", "\u{2591}\u{2592}\u{2593}\u{2588}", " more"];

/// Columns kept clear between the grid and the legend, and between the
/// legend and the edge, where it lines up with the toolbar's right side.
const LEGEND_GAP: u16 = 2;
const LEGEND_MARGIN: u16 = 1;

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn legend_width() -> u16 {
    u16::try_from(
        LEGEND
            .iter()
            .map(|part| Span::raw(*part).width())
            .sum::<usize>(),
    )
    .unwrap_or(u16::MAX)
}

pub(super) fn render_punchcard(events: &VecDeque<Arc<Event>>, frame: &mut Frame, area: Rect) {
    let buckets = event_punchcard(events, &Local);
    let max_count = buckets
        .iter()
        .flatten()
        .copied()
        .max()
        .unwrap_or_default()
        .max(1);
    let (width, legend) = hour_width(area.width);

    let mut lines = vec![axis_line(width, legend.then_some(area.width))];
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

/// The hour axis, with the density scale against the right edge of an
/// area `legend_in` columns wide when there is room for it.
fn axis_line(width: u16, legend_in: Option<u16>) -> Line<'static> {
    let muted = Style::default().fg(theme::text_muted());
    let axis = hour_axis(width);
    let Some(area_width) = legend_in else {
        return Line::from(Span::styled(axis, muted));
    };
    let start = usize::from(area_width.saturating_sub(legend_width() + LEGEND_MARGIN));
    let colors = theme::event_density_colors();
    let [less, cells, more] = LEGEND;
    let mut spans = vec![
        Span::styled(format!("{axis:<start$}"), muted),
        Span::styled(less, muted),
    ];
    let shades = [theme::text_muted(), colors[0], colors[1], colors[2]];
    for (cell, color) in cells.chars().zip(shades) {
        spans.push(Span::styled(cell.to_string(), Style::default().fg(color)));
    }
    spans.push(Span::styled(more, muted));
    Line::from(spans)
}

/// Cells per hour, and whether the legend fits beside the grid. The room
/// right of the gutter is split 24 ways, from one cell up to
/// `MAX_HOUR_WIDTH`; the legend takes its room first wherever that still
/// leaves cells wide enough to keep their gaps.
fn hour_width(area_width: u16) -> (u16, bool) {
    let cells = |room: u16| (room.saturating_sub(GUTTER) / 24).clamp(1, MAX_HOUR_WIDTH);
    let legend = legend_width() + LEGEND_GAP + LEGEND_MARGIN;
    let beside = cells(area_width.saturating_sub(legend));
    if beside >= MIN_HOUR_WIDTH_BESIDE_LEGEND {
        (beside, true)
    } else {
        (cells(area_width), false)
    }
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

fn event_punchcard(events: &VecDeque<Arc<Event>>, timezone: &impl TimeZone) -> [[usize; 24]; 7] {
    let mut buckets = [[0usize; 24]; 7];
    for event in events {
        let local = event.timestamp.with_timezone(timezone);
        let day = usize::try_from(local.weekday().num_days_from_monday()).unwrap_or(0);
        let hour = usize::try_from(local.hour()).unwrap_or(0);
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
    use super::{GUTTER, MAX_HOUR_WIDTH, axis_line, hour_axis, hour_cell, hour_width};

    #[test]
    fn local_offset_moves_events_to_the_previous_weekday() {
        let event = serde_json::from_value(serde_json::json!({
            "timestamp": "2026-11-02T01:30:00Z", "category": "Client",
            "severity": "Info", "event_type": "connected", "message": "connected"
        }))
        .expect("event");
        let events = std::collections::VecDeque::from([std::sync::Arc::new(event)]);
        let timezone = chrono::FixedOffset::west_opt(8 * 3600).expect("offset");
        let buckets = super::event_punchcard(&events, &timezone);
        assert_eq!(buckets[6][17], 1);
        assert_eq!(buckets[0][1], 0);
        assert_eq!(buckets.iter().flatten().sum::<usize>(), 1);
    }

    #[test]
    fn hour_cells_fill_the_room_the_gutter_and_the_legend_leave() {
        assert_eq!(hour_width(181), (6, true), "183-column terminal");
        assert_eq!(hour_width(101), (3, true), "103-column terminal");
        assert_eq!(hour_width(98), (3, true), "100-column terminal");
        // The legend goes before the cells lose their gaps to it.
        assert_eq!(hour_width(78), (3, false), "80-column terminal");
        assert_eq!(hour_width(40), (1, false));
        assert_eq!(hour_width(1000), (MAX_HOUR_WIDTH, true));
    }

    #[test]
    fn the_legend_ends_the_axis_row_one_column_in_from_the_edge() {
        for area in [95, 101, 181] {
            let (width, legend) = hour_width(area);
            assert!(legend, "{area}");
            let row = axis_line(width, Some(area)).to_string();
            assert!(
                row.ends_with("less \u{2591}\u{2592}\u{2593}\u{2588} more"),
                "{row}"
            );
            assert_eq!(row.chars().count(), usize::from(area) - 1, "{row}");
            // The last hour label keeps clear of it.
            assert!(row.contains("  less"), "{row}");
        }
        let bare = axis_line(3, None).to_string();
        assert!(!bare.contains("less"), "{bare}");
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
