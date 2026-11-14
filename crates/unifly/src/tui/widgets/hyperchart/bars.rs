//! Ranked horizontal bar list widget.
//!
//! Renders a list of labelled rows with filled-block bars sized proportionally
//! to either the max-observed value ([`Denominator::MaxObserved`]) or the sum
//! of all values ([`Denominator::Total`]). Handles byte formatting, percentage
//! formatting, or plain integer counts.
//!
//! Replaces hand-rolled `Paragraph`-of-`Line` loops in the stats screen with a
//! single reusable widget that honours the shared HyperChart look-and-feel.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{block, empty};
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;

/// One row in a ranked bar list.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    pub label: &'a str,
    pub value: u64,
}

/// How to compute each row's bar fill ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denominator {
    /// Bar width = value / max value observed across all rows. The largest
    /// row always fills the full bar budget.
    MaxObserved,
    /// Bar width = value / sum of all values. Best paired with
    /// [`ValueFormat::Percent`] so the value column shows the same fraction.
    Total,
}

/// How to format the numeric value column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueFormat {
    /// Human-readable bytes: `"1.2G"`, `"245M"`.
    Bytes,
    /// Percentage of the total: `"42%"`. Pair with [`Denominator::Total`].
    Percent,
    /// Plain integer: `"128"`.
    Count,
}

/// Ranked horizontal bar list widget.
pub struct HyperBars<'a> {
    title: Line<'a>,
    rows: &'a [Row<'a>],
    denominator: Denominator,
    value_format: ValueFormat,
    label_width: usize,
    empty_message: &'a str,
    focused: bool,
    show_rank: bool,
}

impl<'a> HyperBars<'a> {
    /// Construct a new `HyperBars` with sensible defaults (MaxObserved
    /// denominator, Bytes format, 14-character label column, rank numbers on).
    pub fn new(title: Line<'a>, rows: &'a [Row<'a>]) -> Self {
        Self {
            title,
            rows,
            denominator: Denominator::MaxObserved,
            value_format: ValueFormat::Bytes,
            label_width: 14,
            empty_message: "No data",
            focused: false,
            show_rank: true,
        }
    }

    #[must_use]
    pub fn denominator(mut self, denominator: Denominator) -> Self {
        self.denominator = denominator;
        self
    }

    #[must_use]
    pub fn value_format(mut self, format: ValueFormat) -> Self {
        self.value_format = format;
        self
    }

    #[must_use]
    pub fn label_width(mut self, width: usize) -> Self {
        self.label_width = width;
        self
    }

    #[must_use]
    pub fn empty_message(mut self, message: &'a str) -> Self {
        self.empty_message = message;
        self
    }

    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Toggle the leading rank column (`1.`, `2.`, …). On by default.
    #[must_use]
    pub fn show_rank(mut self, show: bool) -> Self {
        self.show_rank = show;
        self
    }
}

impl Widget for HyperBars<'_> {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = block::standard(self.title, self.focused);
        let inner = block.inner(area);
        block.render(area, buf);

        if self.rows.is_empty() {
            empty::render(inner, buf, self.empty_message);
            return;
        }

        let denom_value: u64 = match self.denominator {
            Denominator::MaxObserved => self
                .rows
                .iter()
                .map(|row| row.value)
                .max()
                .unwrap_or(1)
                .max(1),
            Denominator::Total => {
                let total: u64 = self.rows.iter().map(|row| row.value).sum();
                total.max(1)
            }
        };
        let max_rows = usize::from(inner.height);
        let values: Vec<String> = self
            .rows
            .iter()
            .take(max_rows)
            .map(|row| match self.value_format {
                ValueFormat::Bytes => bytes_fmt::fmt_bytes_short(row.value),
                ValueFormat::Percent => {
                    let pct = (row.value as f64 / denom_value as f64) * 100.0;
                    format!("{pct:.0}%")
                }
                ValueFormat::Count => row.value.to_string(),
            })
            .collect();

        // Column widths: leading gap (2) + optional rank (3) + label + space (1) + bar + space (1) + value.
        // The value column grows to its widest value, so a large one keeps
        // its unit instead of losing it to the border.
        let nominal_width: usize = match self.value_format {
            ValueFormat::Bytes => 6,
            ValueFormat::Percent => 4,
            ValueFormat::Count => 7,
        };
        let value_width = values
            .iter()
            .map(|value| Span::raw(value.as_str()).width())
            .fold(nominal_width, usize::max);
        let rank_width: usize = if self.show_rank { 3 } else { 0 };
        let gutter = 2 + rank_width + self.label_width + 1 + 1 + value_width;
        let bar_budget = usize::from(inner.width).saturating_sub(gutter);

        let colors = theme::chart_series();

        let mut lines: Vec<Line> = Vec::with_capacity(max_rows);
        for (idx, row) in self.rows.iter().enumerate().take(max_rows) {
            let bar_width = u16::try_from(bar_budget).unwrap_or(u16::MAX);
            let bar = fractional_bar(row.value, denom_value, bar_width);
            let color = colors[idx % colors.len()];

            let display_label: String = row.label.chars().take(self.label_width).collect();

            let value_str = format!(
                "{:>value_width$}",
                values.get(idx).map_or("", String::as_str)
            );

            let label_width = self.label_width;
            let mut spans: Vec<Span> = Vec::with_capacity(4);
            if self.show_rank {
                spans.push(Span::styled(
                    format!("  {:>2}", idx + 1),
                    Style::default().fg(theme::text_muted()),
                ));
                spans.push(Span::styled(
                    format!(" {display_label:<label_width$} "),
                    Style::default().fg(theme::text_secondary()),
                ));
            } else {
                spans.push(Span::styled(
                    format!("  {display_label:<label_width$} "),
                    Style::default().fg(theme::text_secondary()),
                ));
            }
            spans.push(Span::styled(bar, Style::default().fg(color)));
            spans.push(Span::styled(
                format!(" {value_str}"),
                Style::default().fg(theme::text_secondary()),
            ));
            lines.push(Line::from(spans));
        }

        Paragraph::new(lines).render(inner, buf);
    }
}

fn fractional_bar(value: u64, max_value: u64, width: u16) -> String {
    let bar = bytes_fmt::fmt_traffic_bar(value, max_value, width);
    if value == 0 || bar.chars().any(|ch| ch != ' ') {
        return bar;
    }

    let mut chars: Vec<char> = bar.chars().collect();
    if let Some(first) = chars.first_mut() {
        *first = '▏';
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn render_bars(widget: HyperBars, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        buf
    }

    #[test]
    fn empty_rows_render_empty_message() {
        let rows: &[Row] = &[];
        let widget =
            HyperBars::new(Line::from(" Top Apps "), rows).empty_message("No DPI data available");
        let buf = render_bars(widget, 40, 6);
        // Buffer should contain our empty-state message somewhere.
        let rendered: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .filter_map(|x| buf.cell((x, y)).map(|c| c.symbol().to_string()))
                    .collect::<String>()
            })
            .collect();
        assert!(rendered.contains("No DPI data available"));
    }

    #[test]
    fn max_observed_denominator_fills_largest_row() {
        let dataset = [
            Row {
                label: "Netflix",
                value: 1_000_000_000,
            },
            Row {
                label: "YouTube",
                value: 500_000_000,
            },
        ];
        let widget = HyperBars::new(Line::from(" Apps "), &dataset);
        let buf = render_bars(widget, 60, 6);

        // Row 0 should have the largest bar because MaxObserved normalises
        // against the top row's value.
        let first_row: String = (0..buf.area().width)
            .filter_map(|x| buf.cell((x, 1)).map(|c| c.symbol().to_string()))
            .collect();
        let second_row: String = (0..buf.area().width)
            .filter_map(|x| buf.cell((x, 2)).map(|c| c.symbol().to_string()))
            .collect();
        let first_row_bars = first_row.matches('█').count();
        let second_row_bars = second_row.matches('█').count();
        assert!(
            first_row_bars > second_row_bars,
            "first row bars ({first_row_bars}) should exceed second row bars ({second_row_bars})"
        );
    }

    #[test]
    fn total_denominator_with_percent_format_sums_near_100() {
        let rows = [
            Row {
                label: "Streaming",
                value: 70,
            },
            Row {
                label: "Web",
                value: 30,
            },
        ];
        let widget = HyperBars::new(Line::from(" Categories "), &rows)
            .denominator(Denominator::Total)
            .value_format(ValueFormat::Percent)
            .label_width(12);
        let buf = render_bars(widget, 50, 6);

        let rendered: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .filter_map(|x| buf.cell((x, y)).map(|c| c.symbol().to_string()))
                    .collect::<String>()
            })
            .collect();
        assert!(rendered.contains("70%"), "expected 70% in rendered output");
        assert!(rendered.contains("30%"), "expected 30% in rendered output");
    }

    #[test]
    fn fractional_bar_preserves_tiny_non_zero_values() {
        let bar = fractional_bar(1, 1_000, 8);

        assert!(bar.starts_with('▏'));
        assert_eq!(bar.chars().count(), 8);
    }
}
