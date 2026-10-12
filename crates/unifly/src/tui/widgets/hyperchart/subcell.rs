//! Sub-cell area-fill compositor for cell-native HyperChart rendering.
//!
//! Composes filled area charts from eighth-block glyphs instead of braille
//! dot columns: full cells carry gradient band colors, and the boundary cell
//! carries a partial block in the series line color so the fill edge doubles
//! as the stroke at 8x vertical resolution. Down-direction series fake
//! upper-partial blocks (which do not exist in the basic Block Elements
//! range) by swapping foreground and background on the complementary lower
//! block. Block Elements are universally supported, so this path works on
//! every glyph tier.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use super::model::{Baseline, FillStyle, SeriesDirection};
use super::scene::{ChartScene, SceneSeries};
use crate::tui::render_caps::RenderCaps;
use crate::tui::theme;

const LOWER_BLOCKS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
const EIGHTHS_PER_ROW: usize = 8;

/// Render the scene with the sub-cell compositor. Returns `false` when the
/// scene shape is not supported so the caller can fall back to the canvas
/// renderer.
pub(super) fn render(
    scene: &ChartScene<'_>,
    area: Rect,
    buf: &mut Buffer,
    caps: RenderCaps,
) -> bool {
    if area.width == 0 || area.height == 0 || !supports(scene) {
        return false;
    }

    buf.set_style(area, Style::default().bg(theme::bg_base()));

    let total_eighths = usize::from(area.height) * EIGHTHS_PER_ROW;
    let baseline = snapped_baseline_eighth(scene, total_eighths);
    for series in &scene.series {
        render_series(scene, series, area, buf, caps, baseline, total_eighths);
    }
    true
}

/// Supported shapes: every series filled, with at most one series per
/// baseline side. Overlapping fills on the same side cannot composite in
/// cell space, so those scenes keep the line-oriented canvas renderer.
fn supports(scene: &ChartScene<'_>) -> bool {
    if scene.series.is_empty()
        || scene
            .series
            .iter()
            .any(|series| matches!(series.fill, FillStyle::None))
    {
        return false;
    }

    let ups = scene
        .series
        .iter()
        .filter(|series| series.direction == SeriesDirection::Up)
        .count();
    let downs = scene.series.len() - ups;
    match scene.baseline {
        Baseline::Zero { .. } => ups == 1 && downs == 0,
        Baseline::Mirror { .. } => ups <= 1 && downs <= 1,
    }
}

/// Baseline snapped to a cell boundary so mirrored halves never share a
/// cell; a shared cell would need two partial fills with different colors,
/// which a single glyph cannot express.
fn snapped_baseline_eighth(scene: &ChartScene<'_>, total_eighths: usize) -> usize {
    let raw = eighth_of(scene, total_eighths, 0.0);
    let row = (raw + EIGHTHS_PER_ROW / 2) / EIGHTHS_PER_ROW;
    (row * EIGHTHS_PER_ROW).min(total_eighths)
}

/// Distance from the plot top in eighth-of-a-cell units.
fn eighth_of(scene: &ChartScene<'_>, total_eighths: usize, y: f64) -> usize {
    let bounds = scene.bounds;
    let span = (bounds.y_max - bounds.y_min).max(f64::EPSILON);
    let ratio = ((bounds.y_max - y) / span).clamp(0.0, 1.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    {
        ((total_eighths as f64) * ratio).round() as usize
    }
}

fn render_series(
    scene: &ChartScene<'_>,
    series: &SceneSeries<'_>,
    area: Rect,
    buf: &mut Buffer,
    caps: RenderCaps,
    baseline: usize,
    total_eighths: usize,
) {
    let Some(bands) = series.fill.bands(caps, usize::from(area.height)) else {
        return;
    };

    for (column, value) in sample_columns(scene, series, area.width)
        .into_iter()
        .enumerate()
    {
        let Some(value) = value else {
            continue;
        };
        let edge = eighth_of(scene, total_eighths, scene.transform_y(series, value));
        match series.direction {
            SeriesDirection::Up => {
                // Hold a one-eighth floor so live-but-idle series keep a
                // visible heartbeat line instead of vanishing.
                let top = edge.min(baseline.saturating_sub(1));
                paint_up_column(area, buf, column, top, baseline, series.line_color, &bands);
            }
            SeriesDirection::Down => {
                let bottom = edge.max((baseline + 1).min(total_eighths));
                paint_down_column(
                    area,
                    buf,
                    column,
                    baseline,
                    bottom,
                    series.line_color,
                    &bands,
                );
            }
        }
    }
}

/// Fill `[top, baseline)` eighths growing upward from the baseline. The
/// boundary cell is a lower block in the line color: the fill edge is the
/// stroke.
fn paint_up_column(
    area: Rect,
    buf: &mut Buffer,
    column: usize,
    top: usize,
    baseline: usize,
    line_color: Color,
    bands: &[Color],
) {
    let span = baseline.saturating_sub(top).max(1);
    for row in (top / EIGHTHS_PER_ROW)..baseline.div_ceil(EIGHTHS_PER_ROW) {
        let cell_top = row * EIGHTHS_PER_ROW;
        let filled = (cell_top + EIGHTHS_PER_ROW).min(baseline) - cell_top.max(top);
        if filled == 0 {
            continue;
        }
        let style = if filled == EIGHTHS_PER_ROW {
            let distance = baseline - (cell_top + EIGHTHS_PER_ROW / 2);
            Style::default().fg(band_color(bands, distance, span))
        } else {
            Style::default().fg(line_color)
        };
        put(area, buf, column, row, glyph(filled), style);
    }
}

/// Fill `[baseline, bottom)` eighths growing downward. The boundary cell is
/// top-filled, faked by drawing the complementary lower block with swapped
/// foreground and background.
fn paint_down_column(
    area: Rect,
    buf: &mut Buffer,
    column: usize,
    baseline: usize,
    bottom: usize,
    line_color: Color,
    bands: &[Color],
) {
    let span = bottom.saturating_sub(baseline).max(1);
    for row in (baseline / EIGHTHS_PER_ROW)..bottom.div_ceil(EIGHTHS_PER_ROW) {
        let cell_top = row * EIGHTHS_PER_ROW;
        let filled = (cell_top + EIGHTHS_PER_ROW).min(bottom) - cell_top.max(baseline);
        if filled == 0 {
            continue;
        }
        if filled == EIGHTHS_PER_ROW {
            let distance = (cell_top + EIGHTHS_PER_ROW / 2) - baseline;
            let style = Style::default().fg(band_color(bands, distance, span));
            put(area, buf, column, row, glyph(EIGHTHS_PER_ROW), style);
        } else {
            let style = Style::default().fg(theme::bg_base()).bg(line_color);
            put(
                area,
                buf,
                column,
                row,
                glyph(EIGHTHS_PER_ROW - filled),
                style,
            );
        }
    }
}

/// Per-column series magnitude: interpolation at the column center, raised
/// by any raw sample that lands in the column so single-sample spikes
/// survive downsampling. Columns outside every segment stay `None` (gaps).
fn sample_columns(
    scene: &ChartScene<'_>,
    series: &SceneSeries<'_>,
    width: u16,
) -> Vec<Option<f64>> {
    let bounds = scene.bounds;
    let span = (bounds.x_max - bounds.x_min).max(f64::EPSILON);
    let columns = usize::from(width);
    let mut values: Vec<Option<f64>> = vec![None; columns];

    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let center_x = |column: usize| bounds.x_min + (column as f64 + 0.5) * span / columns as f64;

    for segment in series.data.visible_segments() {
        let (Some(&(first_x, _)), Some(&(last_x, _))) = (segment.first(), segment.last()) else {
            continue;
        };

        for (column, slot) in values.iter_mut().enumerate() {
            let x = center_x(column);
            if x < first_x || x > last_x {
                continue;
            }
            let value = interpolate(&segment, x);
            *slot = Some(slot.map_or(value, |current| current.max(value)));
        }

        for &(x, y) in &segment {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            let column = (((x - bounds.x_min) / span * columns as f64).floor() as usize)
                .min(columns.saturating_sub(1));
            if let Some(slot) = values.get_mut(column) {
                *slot = Some(slot.map_or(y, |current| current.max(y)));
            }
        }
    }

    values
}

fn interpolate(segment: &[(f64, f64)], x: f64) -> f64 {
    let index = segment.partition_point(|&(point_x, _)| point_x < x);
    match (
        index.checked_sub(1).and_then(|left| segment.get(left)),
        segment.get(index),
    ) {
        (Some(&(x0, y0)), Some(&(x1, y1))) => {
            let dx = x1 - x0;
            if dx.abs() < f64::EPSILON {
                y0.max(y1)
            } else {
                y0 + (y1 - y0) * ((x - x0) / dx)
            }
        }
        (Some(&(_, y)), None) | (None, Some(&(_, y))) => y,
        (None, None) => 0.0,
    }
}

fn band_color(bands: &[Color], distance: usize, span: usize) -> Color {
    let last = bands.len().saturating_sub(1);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    let index = ((distance as f64 / span as f64) * last as f64).round() as usize;
    bands.get(index.min(last)).copied().unwrap_or(Color::Reset)
}

fn glyph(filled: usize) -> &'static str {
    LOWER_BLOCKS
        .get(filled.min(EIGHTHS_PER_ROW))
        .copied()
        .unwrap_or("█")
}

fn put(area: Rect, buf: &mut Buffer, column: usize, row: usize, symbol: &str, style: Style) {
    let Ok(x_offset) = u16::try_from(column) else {
        return;
    };
    let Ok(y_offset) = u16::try_from(row) else {
        return;
    };
    if x_offset >= area.width || y_offset >= area.height {
        return;
    }
    if let Some(cell) = buf.cell_mut((area.x + x_offset, area.y + y_offset)) {
        cell.set_symbol(symbol);
        cell.set_style(style);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    use super::*;
    use crate::tui::render_caps::{ColorDepth, GlyphTier, GraphicsProtocol};
    use crate::tui::widgets::hyperchart::ChartGradient;
    use crate::tui::widgets::hyperchart::model::{Baseline, SeriesData, XAxis};
    use crate::tui::widgets::hyperchart::scene::{GridSpec, PlotBounds, mirror_lower_extent};

    fn caps() -> RenderCaps {
        RenderCaps {
            color_depth: ColorDepth::TrueColor,
            glyph_tier: GlyphTier::Block,
            graphics_protocol: GraphicsProtocol::None,
        }
    }

    fn up_series(data: &[(f64, f64)]) -> SceneSeries<'_> {
        SceneSeries {
            name: "RX",
            data: SeriesData::Dense(data),
            line_color: Color::Cyan,
            fill: FillStyle::Gradient(ChartGradient::new(Color::Black, Color::Blue)),
            direction: SeriesDirection::Up,
        }
    }

    fn zero_scene(series: Vec<SceneSeries<'_>>, y_max: f64, x_max: f64) -> ChartScene<'_> {
        ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max,
                y_min: 0.0,
                y_max,
            },
            baseline: Baseline::Zero { y_max },
            series,
            grid: GridSpec { tick_count: 4 },
            annotations: Vec::new(),
        }
    }

    fn symbol_at(buf: &Buffer, x: u16, y: u16) -> String {
        buf.cell((x, y))
            .map_or_else(String::new, |cell| cell.symbol().to_string())
    }

    #[test]
    fn unfilled_series_is_unsupported() {
        let data = [(0.0, 1.0), (1.0, 2.0)];
        let mut series = vec![up_series(&data)];
        series[0].fill = FillStyle::None;
        let scene = zero_scene(series, 4.0, 1.0);
        let area = Rect::new(0, 0, 8, 4);
        let mut buf = Buffer::empty(area);

        assert!(!render(&scene, area, &mut buf, caps()));
    }

    #[test]
    fn full_value_column_fills_to_the_top() {
        let data = [(0.0, 4.0), (1.0, 4.0)];
        let scene = zero_scene(vec![up_series(&data)], 4.0, 1.0);
        let area = Rect::new(0, 0, 4, 4);
        let mut buf = Buffer::empty(area);

        assert!(render(&scene, area, &mut buf, caps()));
        for y in 0..4 {
            assert_eq!(symbol_at(&buf, 1, y), "█", "row {y} should be solid");
        }
    }

    #[test]
    fn half_value_boundary_cell_is_partial_in_line_color() {
        let data = [(0.0, 2.25), (1.0, 2.25)];
        let scene = zero_scene(vec![up_series(&data)], 4.0, 1.0);
        let area = Rect::new(0, 0, 4, 4);
        let mut buf = Buffer::empty(area);

        assert!(render(&scene, area, &mut buf, caps()));
        // 2.25 of 4.0 over 32 eighths => edge at eighth 14, boundary row 1
        // carries a 2-eighth partial.
        assert_eq!(symbol_at(&buf, 1, 1), "▂");
        let boundary = buf.cell((1, 1)).map(|cell| cell.fg);
        assert_eq!(boundary, Some(Color::Cyan));
        assert_eq!(symbol_at(&buf, 1, 2), "█");
        assert_eq!(symbol_at(&buf, 1, 3), "█");
        assert_eq!(symbol_at(&buf, 1, 0), " ");
    }

    #[test]
    fn zero_value_keeps_a_heartbeat_floor() {
        let data = [(0.0, 0.0), (1.0, 0.0)];
        let scene = zero_scene(vec![up_series(&data)], 4.0, 1.0);
        let area = Rect::new(0, 0, 4, 4);
        let mut buf = Buffer::empty(area);

        assert!(render(&scene, area, &mut buf, caps()));
        assert_eq!(symbol_at(&buf, 1, 3), "▁");
    }

    #[test]
    fn gap_columns_stay_empty() {
        let points = [
            crate::tui::widgets::hyperchart::ChartPoint {
                x: 0.0,
                y: Some(3.0),
            },
            crate::tui::widgets::hyperchart::ChartPoint { x: 5.0, y: None },
            crate::tui::widgets::hyperchart::ChartPoint {
                x: 10.0,
                y: Some(3.0),
            },
        ];
        let series = SceneSeries {
            data: SeriesData::Gapped(&points),
            ..up_series(&[])
        };
        let scene = zero_scene(vec![series], 4.0, 10.0);
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = Buffer::empty(area);

        assert!(render(&scene, area, &mut buf, caps()));
        for y in 0..4 {
            assert_eq!(symbol_at(&buf, 5, y), " ", "gap column row {y}");
        }
    }

    #[test]
    fn single_sample_spike_survives_column_sampling() {
        let data = [(0.0, 0.5), (4.6, 4.0), (10.0, 0.5)];
        let scene = zero_scene(vec![up_series(&data)], 4.0, 10.0);
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = Buffer::empty(area);

        assert!(render(&scene, area, &mut buf, caps()));
        assert_eq!(symbol_at(&buf, 4, 0), "█", "spike column should reach top");
    }

    #[test]
    fn mirror_halves_compose_without_sharing_cells() {
        let rx = [(0.0, 90.0), (10.0, 90.0)];
        let tx = [(0.0, 25.0), (10.0, 25.0)];
        let series = vec![
            up_series(&rx),
            SceneSeries {
                name: "TX",
                data: SeriesData::Dense(&tx),
                line_color: Color::Magenta,
                fill: FillStyle::Gradient(ChartGradient::new(Color::Black, Color::Red)),
                direction: SeriesDirection::Down,
            },
        ];
        let scene = ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max: 10.0,
                y_min: -mirror_lower_extent(90.0),
                y_max: 90.0,
            },
            baseline: Baseline::Mirror {
                upper_max: 90.0,
                lower_max: 30.0,
                upper_label: "RX",
                lower_label: "TX",
            },
            series,
            grid: GridSpec { tick_count: 4 },
            annotations: Vec::new(),
        };
        let area = Rect::new(0, 0, 10, 9);
        let mut buf = Buffer::empty(area);

        assert!(render(&scene, area, &mut buf, caps()));
        // The lanes split 2:1, so the baseline lands on eighth 48, a cell
        // boundary at row 6. RX fills rows 0..6 solid; TX 25 of 30 covers
        // 20 of the lower lane's 24 eighths, so rows 6 and 7 are solid and
        // row 8 is a 4-eighth top fill via the inversion trick.
        assert_eq!(symbol_at(&buf, 3, 0), "█");
        assert_eq!(symbol_at(&buf, 3, 5), "█");
        assert_eq!(symbol_at(&buf, 3, 6), "█");
        assert_eq!(symbol_at(&buf, 3, 7), "█");
        let bottom = buf
            .cell((3, 8))
            .map(|cell| (cell.symbol().to_string(), cell.bg));
        assert_eq!(bottom, Some(("▄".to_string(), Color::Magenta)));
    }
}
