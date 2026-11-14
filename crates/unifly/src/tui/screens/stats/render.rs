use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use super::{
    BANDWIDTH_LABEL_WIDTH, BANDWIDTH_TICK_COUNT, CLIENT_LABEL_WIDTH, CLIENT_TICK_COUNT,
    MIN_BANDWIDTH_SCALE, StatsScreen,
};
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;
use crate::tui::widgets::click::{self, ClickLine, Hint};
use crate::tui::widgets::hyperchart::{
    Baseline, ChartGradient, Denominator, Domain, FillStyle, HyperBars, HyperChart, Renderer, Row,
    Series, SeriesData, SeriesDirection, ValueFormat, XAxis,
};
use crate::tui::widgets::{sub_tabs, toolbar, wrap};

const STATS_BANDWIDTH_GUTTER: u16 = 7;
const STATS_CLIENT_GUTTER: u16 = 6;

/// Rows the client chart keeps when the category list wants more: its
/// frame and the seven rows from which it draws a time axis.
const CLIENT_CHART_ROWS: u16 = 9;

impl StatsScreen {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    pub(super) fn render_bandwidth_chart(&self, frame: &mut Frame, area: Rect) {
        let tx_y_max = self.bandwidth_tx_y_max.max(MIN_BANDWIDTH_SCALE);
        let rx_y_max = self.bandwidth_rx_y_max.max(MIN_BANDWIDTH_SCALE);
        let x_bounds = bandwidth_x_bounds(&self.bandwidth_tx, &self.bandwidth_rx);
        let (rx_start, rx_end) = theme::rx_gradient_endpoints();
        let (tx_start, tx_end) = theme::tx_gradient_endpoints();

        let current_tx = self.bandwidth_tx.last().map_or(0, |&(_, v)| v as u64);
        let current_rx = self.bandwidth_rx.last().map_or(0, |&(_, v)| v as u64);
        let peak = self
            .bandwidth_tx
            .iter()
            .chain(self.bandwidth_rx.iter())
            .map(|&(_, v)| v)
            .fold(0.0_f64, f64::max) as u64;

        let title = Line::from(vec![
            Span::styled(" WAN Bandwidth ", theme::title_style()),
            Span::styled("── ", Style::default().fg(theme::border_unfocused())),
            Span::styled(
                format!("TX {} ↑", bytes_fmt::fmt_rate(current_tx)),
                Style::default().fg(theme::accent_secondary()),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("RX {} ↓", bytes_fmt::fmt_rate(current_rx)),
                Style::default().fg(theme::accent_tertiary()),
            ),
            Span::styled(
                format!("  Peak {} ", bytes_fmt::fmt_rate(peak)),
                Style::default().fg(theme::border_unfocused()),
            ),
        ]);

        let series = [
            Series {
                name: "RX",
                data: SeriesData::Dense(&self.bandwidth_rx),
                line_color: theme::accent_tertiary(),
                fill: FillStyle::Gradient(ChartGradient::new(rx_start, rx_end)),
                direction: SeriesDirection::Up,
            },
            Series {
                name: "TX",
                data: SeriesData::Dense(&self.bandwidth_tx),
                line_color: theme::accent_secondary(),
                fill: FillStyle::Gradient(ChartGradient::new(tx_start, tx_end)),
                direction: SeriesDirection::Down,
            },
        ];

        let chart = HyperChart::new(title, &series, x_bounds, rx_y_max)
            .domain(Domain::Rate)
            .x_axis(XAxis::Epoch)
            .baseline(Baseline::Mirror {
                upper_max: rx_y_max,
                lower_max: tx_y_max,
                upper_label: "RX",
                lower_label: "TX",
            })
            .tick_count(BANDWIDTH_TICK_COUNT)
            .label_width(BANDWIDTH_LABEL_WIDTH)
            .renderer(Renderer::Canvas {
                gutter_width: STATS_BANDWIDTH_GUTTER,
            })
            .empty_message("No bandwidth data yet");

        frame.render_widget(chart, area);
    }

    pub(super) fn render_client_chart(&self, frame: &mut Frame, area: Rect) {
        let y_max = self.client_y_max.max(1.0);
        let x_bounds = self
            .client_counts
            .first()
            .zip(self.client_counts.last())
            .map_or((0.0, 1.0), |((x_min, _), (x_max, _))| (*x_min, *x_max));

        let series = [Series {
            name: "Clients",
            data: SeriesData::Dense(&self.client_counts),
            line_color: theme::accent_primary(),
            fill: {
                let (start, end) = theme::client_gradient_endpoints();
                FillStyle::Gradient(ChartGradient::new(start, end))
            },
            direction: SeriesDirection::Up,
        }];

        let chart = HyperChart::new(Line::from(" Client Count "), &series, x_bounds, y_max)
            .domain(Domain::Count)
            .x_axis(XAxis::Epoch)
            .tick_count(CLIENT_TICK_COUNT)
            .label_width(CLIENT_LABEL_WIDTH)
            .renderer(Renderer::Canvas {
                gutter_width: STATS_CLIENT_GUTTER,
            })
            .empty_message("No client data yet");

        frame.render_widget(chart, area);
    }

    pub(super) fn render_top_apps(&self, frame: &mut Frame, area: Rect) {
        let rows: Vec<Row> = self
            .dpi_apps
            .iter()
            .map(|(name, bytes)| Row {
                label: name.as_str(),
                value: *bytes,
            })
            .collect();
        // Wide enough for the longest name, so two applications never
        // read the same, within a third of the panel.
        let longest = rows
            .iter()
            .map(|row| Span::raw(row.label).width())
            .max()
            .unwrap_or(0);
        let label_width = longest.clamp(8, usize::from(area.width / 3).max(8));
        let empty = self.dpi_empty_message("No application traffic recorded");

        let widget = HyperBars::new(Line::from(" Top Applications \u{00B7} all time "), &rows)
            .denominator(Denominator::MaxObserved)
            .value_format(ValueFormat::Bytes)
            .label_width(label_width)
            .empty_message(&empty);

        frame.render_widget(widget, area);
    }

    pub(super) fn render_categories(&self, frame: &mut Frame, area: Rect) {
        let rows: Vec<Row> = self
            .dpi_categories
            .iter()
            .map(|(name, bytes)| Row {
                label: name.as_str(),
                value: *bytes,
            })
            .collect();
        let empty = self.dpi_empty_message("No category traffic recorded");

        let widget = HyperBars::new(Line::from(" Categories \u{00B7} all time "), &rows)
            .denominator(Denominator::Total)
            .value_format(ValueFormat::Percent)
            .label_width(12)
            .empty_message(&empty);

        frame.render_widget(widget, area);
    }

    /// What an empty DPI panel says: the reason the read failed, or that
    /// the controller has counted nothing.
    fn dpi_empty_message(&self, nothing: &str) -> String {
        self.dpi_error.as_ref().map_or_else(
            || nothing.to_owned(),
            |error| format!("Unavailable: {error}"),
        )
    }

    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        let block = Block::default()
            .title(" Statistics ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let [toolbar_row, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);

        let periods = sub_tabs::sub_tab_strip(&PERIOD_LABELS, self.period_index());
        let room = usize::from(toolbar_row.width).saturating_sub(periods.width() + 3);
        let mut status = ClickLine::default();
        if let Some(span) = self.status_span(room) {
            status.push_gap(span);
        }
        toolbar::render(
            frame,
            toolbar_row,
            periods,
            status,
            &mut self.hits.borrow_mut(),
        );

        match self.body_notice() {
            Some((message, color)) => render_notice(frame, body, &message, color),
            None => self.render_charts(frame, body),
        }
    }

    /// What the status bar offers: a reload, where there is a session to
    /// reload from. The periods are their own buttons.
    pub(super) fn status_hints(&self) -> Vec<(u8, Hint)> {
        if self.blocked().is_some() {
            return Vec::new();
        }
        vec![(0, Hint::button("r", "reload", click::char_key('r')))]
    }

    fn render_charts(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([Constraint::Percentage(47), Constraint::Min(8)]).split(area);
        self.render_bandwidth_chart(frame, layout[0]);

        let bottom = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(layout[1]);
        // The category list takes the rows it has, down to what leaves
        // the client chart its plot, and the chart takes the rest.
        let column = bottom[0].height;
        let chart_floor = CLIENT_CHART_ROWS.min(column.div_ceil(2));
        let listed = u16::try_from(self.dpi_categories.len().max(1)).unwrap_or(u16::MAX);
        let categories = listed
            .saturating_add(2)
            .min(column.saturating_sub(chart_floor));
        let left_col =
            Layout::vertical([Constraint::Min(0), Constraint::Length(categories)]).split(bottom[0]);

        self.render_client_chart(frame, left_col[0]);
        self.render_categories(frame, left_col[1]);
        self.render_top_apps(frame, bottom[1]);
    }

    /// What stands in for the charts while they would mislead: why nothing
    /// can load, or that the period on the chip is loading or failed.
    fn body_notice(&self) -> Option<(String, Color)> {
        let label = PERIOD_LABELS[self.period_index()];
        if let Some(reason) = self.blocked() {
            return Some((reason.to_owned(), theme::text_muted()));
        }
        if self.shown == Some(self.period) {
            return None;
        }
        Some(match &self.error {
            Some(error) => (
                format!("Could not load {label}: {error}. Press r to try again."),
                theme::error(),
            ),
            None => (format!("Loading {label}\u{2026}"), theme::text_muted()),
        })
    }

    /// The note against the right edge of the period row while charts are
    /// on screen: a reload in flight, or one that failed and left older
    /// data showing, cut to the `room` beside the periods.
    fn status_span(&self, room: usize) -> Option<Span<'static>> {
        if self.blocked().is_some() || self.shown != Some(self.period) {
            return None;
        }
        if let Some(error) = &self.error {
            let text = format!("\u{2717} reload failed, showing older data: {error}");
            let lines = wrap::wrap_clamped(&text, room.max(8), 1);
            return Some(Span::styled(
                lines.into_iter().next().unwrap_or_default(),
                Style::default().fg(theme::error()),
            ));
        }
        self.loading.then(|| {
            Span::styled(
                "\u{25D0} reloading\u{2026}",
                Style::default().fg(theme::warning()),
            )
        })
    }
}

/// The period chips, in [`crate::tui::action::StatsPeriod::ALL`] order.
const PERIOD_LABELS: [&str; 4] = ["1h", "24h", "7d", "30d"];

/// A notice centered in `area`, wrapped to its width.
fn render_notice(frame: &mut Frame, area: Rect, message: &str, color: Color) {
    let lines = wrap::wrap(message, usize::from(area.width.saturating_sub(4)).max(1));
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let rows = Rect {
        y: area.y + (area.height - height) / 2,
        height,
        ..area
    };
    let lines: Vec<Line> = lines.into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(color))
            .alignment(Alignment::Center),
        rows,
    );
}

/// Compute safe x-axis bounds from two datasets, avoiding collapse when
/// the span is zero (single point or identical start/end x values).
fn bandwidth_x_bounds(tx: &[(f64, f64)], rx: &[(f64, f64)]) -> (f64, f64) {
    let first = tx
        .first()
        .map(|&(x, _)| x)
        .into_iter()
        .chain(rx.first().map(|&(x, _)| x))
        .reduce(f64::min);
    let last = tx
        .last()
        .map(|&(x, _)| x)
        .into_iter()
        .chain(rx.last().map(|&(x, _)| x))
        .reduce(f64::max);
    match (first, last) {
        (Some(min), Some(max)) => (min, max),
        _ => (0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bandwidth_bounds_empty_data_returns_safe_fallback() {
        let empty: &[(f64, f64)] = &[];
        assert_eq!(bandwidth_x_bounds(empty, empty), (0.0, 1.0));
    }

    #[test]
    fn bandwidth_bounds_uses_widest_span_across_series() {
        let tx: &[(f64, f64)] = &[(10.0, 0.0), (20.0, 0.0)];
        let rx: &[(f64, f64)] = &[(5.0, 0.0), (25.0, 0.0)];
        assert_eq!(bandwidth_x_bounds(tx, rx), (5.0, 25.0));
    }
}
