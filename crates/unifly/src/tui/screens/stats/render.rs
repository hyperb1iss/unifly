use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use super::{
    BANDWIDTH_LABEL_WIDTH, BANDWIDTH_TICK_COUNT, CLIENT_LABEL_WIDTH, CLIENT_TICK_COUNT,
    MIN_BANDWIDTH_SCALE, StatsScreen,
};
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;
use crate::tui::widgets::hyperchart::{
    Baseline, ChartGradient, Denominator, Domain, FillStyle, HyperBars, HyperChart, Renderer, Row,
    Series, SeriesData, SeriesDirection, ValueFormat, XAxis,
};
use crate::tui::widgets::sub_tabs;

const STATS_BANDWIDTH_GUTTER: u16 = 7;
const STATS_CLIENT_GUTTER: u16 = 6;

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

        let widget = HyperBars::new(Line::from(" Top Applications "), &rows)
            .denominator(Denominator::MaxObserved)
            .value_format(ValueFormat::Bytes)
            .label_width(14)
            .empty_message("No DPI data available");

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

        let widget = HyperBars::new(Line::from(" Traffic by Category "), &rows)
            .denominator(Denominator::Total)
            .value_format(ValueFormat::Percent)
            .label_width(12)
            .empty_message("No category data");

        frame.render_widget(widget, area);
    }

    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
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

        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Percentage(45),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(inner);

        let strip = sub_tabs::sub_tab_strip(&["1h", "24h", "7d", "30d"], self.period_index());
        *self.period_targets.borrow_mut() = strip.targets(layout[0]);
        frame.render_widget(Paragraph::new(strip.into_line()), layout[0]);

        self.render_bandwidth_chart(frame, layout[1]);

        let bottom = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(layout[2]);
        let left_col = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(bottom[0]);

        self.render_client_chart(frame, left_col[0]);
        self.render_categories(frame, left_col[1]);
        self.render_top_apps(frame, bottom[1]);

        let hints = Line::from(vec![
            Span::styled("  h ", theme::key_hint_key()),
            Span::styled("1h  ", theme::key_hint()),
            Span::styled("d ", theme::key_hint_key()),
            Span::styled("24h  ", theme::key_hint()),
            Span::styled("w ", theme::key_hint_key()),
            Span::styled("7d  ", theme::key_hint()),
            Span::styled("m ", theme::key_hint_key()),
            Span::styled("30d  ", theme::key_hint()),
            Span::styled("r ", theme::key_hint_key()),
            Span::styled("refresh", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[3]);
    }
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
