use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;
use crate::tui::widgets::hyperchart::{
    Baseline, ChartGradient, Domain, FillStyle, HyperChart, Renderer, Series, SeriesData,
    SeriesDirection, XAxis,
};

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::super::{
    BANDWIDTH_GUTTER_WIDTH, BANDWIDTH_LABEL_WIDTH, BANDWIDTH_TICK_COUNT,
    LIVE_CHART_SAMPLE_INTERVAL, LIVE_CHART_WINDOW_SAMPLES, MIN_BANDWIDTH_SCALE,
};

impl DashboardScreen {
    /// Hero panel: WAN traffic chart with Octant-rendered area fill and line
    /// overlay, plus a live TX/RX/peak legend in the title bar.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    pub(super) fn render_traffic_chart(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
    ) {
        let now = std::time::Instant::now();
        let dim = Style::default().fg(theme::border_unfocused());
        let mut title = vec![
            self.panel_title(Panel::Traffic, "WAN Traffic"),
            Span::styled("── ", dim),
        ];
        match (self.live_bandwidth(now), self.bandwidth_stale_for(now)) {
            (Some((tx, rx)), _) => {
                title.push(Span::styled(
                    format!("TX {} ↑", bytes_fmt::fmt_rate(tx)),
                    Style::default().fg(theme::accent_secondary()),
                ));
                title.push(Span::raw("  "));
                title.push(Span::styled(
                    format!("RX {} ↓", bytes_fmt::fmt_rate(rx)),
                    Style::default().fg(theme::accent_tertiary()),
                ));
            }
            // A rate that stopped updating is not shown as the rate now.
            (None, Some(age)) => title.push(Span::styled(
                format!(
                    "no rate for {}",
                    bytes_fmt::fmt_uptime(age.as_secs().max(60))
                ),
                Style::default().fg(theme::warning()),
            )),
            (None, None) => title.push(Span::styled("TX ─  RX ─", dim)),
        }
        title.push(Span::styled(
            format!(
                "  Peak {} ",
                bytes_fmt::fmt_rate(self.peak_rx.max(self.peak_tx))
            ),
            dim,
        ));
        let title = Line::from(title);

        let window_span = LIVE_CHART_WINDOW_SAMPLES.saturating_sub(1) as f64;
        let x_max = self.chart_x_max(now);
        let x_min = x_max - window_span;
        let rx_series = Self::series_held_to(&self.bandwidth_rx, x_max);
        let tx_series = Self::series_held_to(&self.bandwidth_tx, x_max);
        let tx_y_max = self.chart_tx_y_max.max(MIN_BANDWIDTH_SCALE);
        let rx_y_max = self.chart_rx_y_max.max(MIN_BANDWIDTH_SCALE);
        let (rx_start, rx_end) = theme::rx_gradient_endpoints();
        let (tx_start, tx_end) = theme::tx_gradient_endpoints();

        let series = [
            Series {
                name: "RX",
                data: SeriesData::Dense(&rx_series),
                line_color: theme::accent_tertiary(),
                fill: FillStyle::Gradient(ChartGradient::new(rx_start, rx_end)),
                direction: SeriesDirection::Up,
            },
            Series {
                name: "TX",
                data: SeriesData::Dense(&tx_series),
                line_color: theme::accent_secondary(),
                fill: FillStyle::Gradient(ChartGradient::new(tx_start, tx_end)),
                direction: SeriesDirection::Down,
            },
        ];

        let chart = HyperChart::new(title, &series, (x_min, x_max), rx_y_max)
            .domain(Domain::Rate)
            .x_axis(XAxis::Relative {
                sample_interval: LIVE_CHART_SAMPLE_INTERVAL,
            })
            .baseline(Baseline::Mirror {
                upper_max: rx_y_max,
                lower_max: tx_y_max,
                upper_label: "RX",
                lower_label: "TX",
            })
            .tick_count(BANDWIDTH_TICK_COUNT)
            .label_width(BANDWIDTH_LABEL_WIDTH)
            .renderer(Renderer::Canvas {
                gutter_width: BANDWIDTH_GUTTER_WIDTH,
            })
            .focused(self.has_focus(Panel::Traffic))
            .empty_message(self.traffic_empty_text());

        frame.render_widget(chart, area);
    }

    /// What the chart says with no samples: that the controller is not
    /// there yet, that it reports no WAN rate, or that one is on its way.
    fn traffic_empty_text(&self) -> &'static str {
        if let Some(waiting) = self.link.waiting() {
            waiting
        } else if self.last_data_update.is_some() && self.current_bandwidth().is_none() {
            "This controller reports no WAN rate"
        } else {
            "Waiting for the first sample\u{2026}"
        }
    }
}
