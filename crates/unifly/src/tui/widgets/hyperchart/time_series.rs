//! Time-series chart widget with terminal-aware rendering.
//!
//! [`HyperChart`] accepts one or more [`Series`], a [`Domain`] for y-axis
//! label formatting, an [`XAxis`] descriptor, and a [`Baseline`] layout. The
//! canvas renderer is the primary path for portfolio-grade WAN charts: it
//! supports theme-native gradients, mirror TX/RX baselines, gridlines, x-axis
//! labels, gap-aware series, and terminal capability fallbacks.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::model::{Baseline, Domain, FillStyle, Series, XAxis};
use super::scene::{
    self, Annotation, AnnotationKind, ChartScene, GridSpec, PlotBounds, SceneSeries,
};
use super::{axis, block, canvas, empty, tiled};
use crate::tui::render_caps::{self, RenderCaps};
use crate::tui::theme;

/// Rendering back-end for [`HyperChart`].
#[derive(Debug, Clone, Copy)]
pub enum Renderer {
    /// Canvas renderer with a manual y-axis gutter.
    Canvas {
        /// Width reserved for y-axis labels on the left of the plot area.
        gutter_width: u16,
    },
    /// Ratatui `Chart` widget with built-in axes. Kept for compact fallback
    /// panels that do not need mirror baselines.
    Tiled,
}

/// Unified time-series chart widget.
pub struct HyperChart<'a> {
    title: Line<'a>,
    series: &'a [Series<'a>],
    domain: Domain,
    x_axis: XAxis,
    x_bounds: (f64, f64),
    baseline: Baseline<'a>,
    renderer: Renderer,
    render_caps: Option<RenderCaps>,
    tick_count: usize,
    label_width: usize,
    empty_message: &'a str,
    focused: bool,
}

impl<'a> HyperChart<'a> {
    /// Construct a new `HyperChart` with sensible defaults.
    pub fn new(
        title: Line<'a>,
        series: &'a [Series<'a>],
        x_bounds: (f64, f64),
        y_max: f64,
    ) -> Self {
        Self {
            title,
            series,
            domain: Domain::Rate,
            x_axis: XAxis::Hidden,
            x_bounds,
            baseline: Baseline::Zero { y_max },
            renderer: Renderer::Tiled,
            render_caps: None,
            tick_count: 4,
            label_width: 6,
            empty_message: "No data",
            focused: false,
        }
    }

    #[must_use]
    pub fn domain(mut self, domain: Domain) -> Self {
        self.domain = domain;
        self
    }

    #[must_use]
    pub fn x_axis(mut self, x_axis: XAxis) -> Self {
        self.x_axis = x_axis;
        self
    }

    #[must_use]
    pub fn baseline(mut self, baseline: Baseline<'a>) -> Self {
        self.baseline = baseline;
        self
    }

    #[must_use]
    pub fn renderer(mut self, renderer: Renderer) -> Self {
        self.renderer = renderer;
        self
    }

    #[must_use]
    pub fn render_caps(mut self, caps: RenderCaps) -> Self {
        self.render_caps = Some(caps);
        self
    }

    #[must_use]
    pub fn tick_count(mut self, tick_count: usize) -> Self {
        self.tick_count = tick_count;
        self
    }

    #[must_use]
    pub fn label_width(mut self, label_width: usize) -> Self {
        self.label_width = label_width;
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

    pub(super) fn caps(&self) -> RenderCaps {
        self.render_caps.unwrap_or_else(render_caps::current)
    }

    pub(super) fn x_axis_kind(&self) -> XAxis {
        self.x_axis
    }

    pub(super) fn tick_count_value(&self) -> usize {
        self.tick_count
    }

    fn is_empty(&self) -> bool {
        self.scene().is_empty()
    }

    fn resolved_x_bounds(&self) -> (f64, f64) {
        let (min, max) = self.x_bounds;
        if (max - min).abs() < f64::EPSILON {
            (min - 0.5, max + 0.5)
        } else {
            (min, max)
        }
    }

    fn plot_bounds(&self) -> PlotBounds {
        let (x_min, x_max) = self.resolved_x_bounds();
        match self.baseline {
            Baseline::Zero { y_max } => PlotBounds {
                x_min,
                x_max,
                y_min: 0.0,
                y_max: y_max.max(1.0),
            },
            Baseline::Mirror { upper_max, .. } => PlotBounds {
                x_min,
                x_max,
                y_min: -scene::mirror_lower_extent(upper_max),
                y_max: upper_max.max(1.0),
            },
        }
    }

    pub(super) fn scene(&self) -> ChartScene<'_> {
        let bounds = self.plot_bounds();
        ChartScene {
            x_axis: self.x_axis,
            bounds,
            baseline: self.baseline,
            series: self
                .series
                .iter()
                .map(|series| SceneSeries {
                    name: series.name,
                    data: series.data,
                    line_color: series.line_color,
                    fill: series.fill,
                    direction: series.direction,
                })
                .collect(),
            grid: GridSpec {
                tick_count: self.tick_count,
            },
            annotations: self.build_annotations(bounds),
        }
    }

    pub(super) fn build_y_labels(&self, max_value: f64) -> Vec<Span<'static>> {
        let axis_style = Style::default().fg(theme::border_unfocused());
        match self.domain {
            Domain::Rate => {
                axis::rate_axis_labels(max_value, self.tick_count, self.label_width, axis_style)
            }
            Domain::Count => {
                axis::count_axis_labels(max_value, self.tick_count, self.label_width, axis_style)
            }
        }
    }

    pub(super) fn format_value(&self, value: f64) -> String {
        match self.domain {
            Domain::Rate => crate::tui::widgets::bytes_fmt::fmt_rate_axis(value),
            Domain::Count => format!("{value:.0}"),
        }
    }

    fn build_annotations(&self, bounds: PlotBounds) -> Vec<Annotation> {
        let mut annotations = Vec::new();
        for series in self.series {
            if let Some(annotation) = self.visible_annotation(series, bounds, AnnotationKind::Now) {
                annotations.push(annotation);
            }
            if let Some(annotation) = self.visible_annotation(series, bounds, AnnotationKind::Peak)
            {
                annotations.push(annotation);
            }
        }
        annotations
    }

    fn visible_annotation(
        &self,
        series: &Series<'_>,
        bounds: PlotBounds,
        kind: AnnotationKind,
    ) -> Option<Annotation> {
        let mut points = series
            .data
            .points()
            .into_iter()
            .filter(|&(x, _)| x >= bounds.x_min && x <= bounds.x_max);

        let point = match kind {
            AnnotationKind::Now => points.next_back(),
            AnnotationKind::Peak => points.max_by(|left, right| left.1.total_cmp(&right.1)),
        };

        point.map(|(x, y)| Annotation {
            kind,
            x,
            y,
            transformed_y: scene::transform_y(self.baseline, series.direction, y),
            color: series.line_color,
        })
    }
}

impl Widget for HyperChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = block::standard(self.title.clone(), self.focused);
        let inner = block.inner(area);
        block.render(area, buf);

        if self.is_empty() {
            empty::render(inner, buf, self.empty_message);
            return;
        }

        match self.renderer {
            Renderer::Tiled => tiled::render(&self, inner, buf),
            Renderer::Canvas { gutter_width } => canvas::render(&self, inner, buf, gutter_width),
        }
    }
}

impl FillStyle {
    pub(super) fn chart_color(self, caps: RenderCaps) -> Option<Color> {
        match self {
            Self::None => None,
            Self::Solid(color) => Some(color),
            Self::Gradient(gradient) => {
                let bands = gradient.bands(caps, 3);
                bands.get(1).copied().or_else(|| bands.first().copied())
            }
        }
    }

    pub(super) fn bands(self, caps: RenderCaps, height: usize) -> Option<Vec<Color>> {
        match self {
            Self::None => None,
            Self::Solid(color) => Some(vec![color]),
            Self::Gradient(gradient) => Some(gradient.bands(caps, height.clamp(2, 10))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    use crate::tui::render_caps::{ColorDepth, GlyphTier};
    use crate::tui::widgets::hyperchart::{ChartGradient, ChartPoint, SeriesData, SeriesDirection};

    fn render_chart(widget: HyperChart<'_>, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        buf
    }

    fn buffer_text(buf: &Buffer) -> String {
        (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .filter_map(|x| buf.cell((x, y)).map(|cell| cell.symbol().to_string()))
                    .collect::<String>()
            })
            .collect()
    }

    fn test_caps(glyph_tier: GlyphTier) -> RenderCaps {
        RenderCaps {
            color_depth: ColorDepth::TrueColor,
            glyph_tier,
            graphics_protocol: crate::tui::render_caps::GraphicsProtocol::None,
        }
    }

    #[test]
    fn empty_series_renders_empty_message() {
        let series: &[Series<'_>] = &[];
        let widget = HyperChart::new(Line::from(" Bandwidth "), series, (0.0, 10.0), 1_000.0)
            .empty_message("No bandwidth data yet");
        let buf = render_chart(widget, 60, 12);

        assert!(buffer_text(&buf).contains("No bandwidth data yet"));
    }

    #[test]
    fn gapped_series_splits_visible_segments() {
        let points = [
            ChartPoint {
                x: 0.0,
                y: Some(1.0),
            },
            ChartPoint {
                x: 1.0,
                y: Some(2.0),
            },
            ChartPoint { x: 2.0, y: None },
            ChartPoint {
                x: 3.0,
                y: Some(4.0),
            },
        ];

        let segments = SeriesData::Gapped(&points).visible_segments();

        assert_eq!(
            segments,
            vec![vec![(0.0, 1.0), (1.0, 2.0)], vec![(3.0, 4.0)]]
        );
    }

    #[test]
    fn relative_x_axis_labels_end_with_now() {
        let data = [(0.0, 1.0), (120.0, 2.0)];
        let series = [Series {
            name: "TX",
            data: SeriesData::Dense(&data),
            line_color: Color::Cyan,
            fill: FillStyle::None,
            direction: SeriesDirection::Up,
        }];
        let widget = HyperChart::new(Line::from(" Bandwidth "), &series, (0.0, 120.0), 2_000.0)
            .x_axis(XAxis::Relative {
                sample_interval: Duration::from_millis(250),
            })
            .renderer(Renderer::Canvas { gutter_width: 7 })
            .render_caps(test_caps(GlyphTier::Braille));
        let buf = render_chart(widget, 80, 16);
        let text = buffer_text(&buf);

        assert!(text.contains("-30s"));
        assert!(text.contains("now"));
    }

    #[test]
    fn epoch_x_axis_labels_render_clock_times() {
        let data = [(3_600.0, 1.0), (7_200.0, 2.0)];
        let series = [Series {
            name: "Clients",
            data: SeriesData::Dense(&data),
            line_color: Color::Cyan,
            fill: FillStyle::None,
            direction: SeriesDirection::Up,
        }];
        let widget = HyperChart::new(Line::from(" Clients "), &series, (3_600.0, 7_200.0), 3.0)
            .domain(Domain::Count)
            .x_axis(XAxis::Epoch)
            .renderer(Renderer::Canvas { gutter_width: 6 })
            .render_caps(test_caps(GlyphTier::Braille));
        let buf = render_chart(widget, 80, 16);
        let text = buffer_text(&buf);

        assert!(text.contains("01:00"));
        assert!(text.contains("02:00"));
    }

    #[test]
    fn tiled_renderer_draws_single_series_without_panic() {
        let data: Vec<(f64, f64)> = (0..20)
            .map(|i| (f64::from(i), f64::from(i * 100)))
            .collect();
        let series = [Series {
            name: "TX",
            data: SeriesData::Dense(&data),
            line_color: Color::Cyan,
            fill: FillStyle::Solid(Color::Blue),
            direction: SeriesDirection::Up,
        }];
        let widget = HyperChart::new(Line::from(" Bandwidth "), &series, (0.0, 20.0), 2_000.0);
        let _ = render_chart(widget, 60, 12);
    }

    #[test]
    fn canvas_renderer_draws_mirror_series_without_panic() {
        let tx: Vec<(f64, f64)> = (0..30)
            .map(|i| (f64::from(i), f64::from(i) * 123.0))
            .collect();
        let rx: Vec<(f64, f64)> = (0..30)
            .map(|i| (f64::from(i), f64::from(i) * 456.0))
            .collect();
        let series = [
            Series {
                name: "RX",
                data: SeriesData::Dense(&rx),
                line_color: Color::Magenta,
                fill: FillStyle::Gradient(ChartGradient::new(Color::Black, Color::Red)),
                direction: SeriesDirection::Up,
            },
            Series {
                name: "TX",
                data: SeriesData::Dense(&tx),
                line_color: Color::Cyan,
                fill: FillStyle::Gradient(ChartGradient::new(Color::Black, Color::Blue)),
                direction: SeriesDirection::Down,
            },
        ];
        let widget = HyperChart::new(Line::from(" WAN Traffic "), &series, (0.0, 30.0), 20_000.0)
            .baseline(Baseline::Mirror {
                upper_max: 20_000.0,
                lower_max: 4_000.0,
                upper_label: "RX",
                lower_label: "TX",
            })
            .renderer(Renderer::Canvas { gutter_width: 7 })
            .render_caps(test_caps(GlyphTier::Octant));
        let _ = render_chart(widget, 80, 16);
    }

    #[test]
    fn gradient_chart_color_uses_representative_mid_band() {
        let color = FillStyle::Gradient(ChartGradient::new(Color::Black, Color::Red))
            .chart_color(test_caps(GlyphTier::Braille))
            .expect("gradient should produce a chart color");

        assert_ne!(color, Color::Black);
    }

    #[test]
    fn block_tier_renders_without_braille_assumptions() {
        let data: Vec<(f64, f64)> = (0..12)
            .map(|i| (f64::from(i), f64::from(i * 100)))
            .collect();
        let series = [Series {
            name: "Clients",
            data: SeriesData::Dense(&data),
            line_color: Color::Green,
            fill: FillStyle::None,
            direction: SeriesDirection::Up,
        }];
        let widget = HyperChart::new(Line::from(" Clients "), &series, (0.0, 12.0), 1_500.0)
            .renderer(Renderer::Canvas { gutter_width: 6 })
            .render_caps(test_caps(GlyphTier::Block));
        let _ = render_chart(widget, 64, 12);
    }
}
