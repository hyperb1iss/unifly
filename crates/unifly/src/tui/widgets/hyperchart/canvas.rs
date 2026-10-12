//! Canvas renderer for hero HyperChart panels.

use chrono::{DateTime, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Paragraph, Widget};

use super::annotations;
use super::axis;
use super::geometry::{fraction, render_text, x_to_col, y_to_row};
use super::model::{Baseline, SeriesDirection, XAxis};
use super::scene::{ChartScene, PlotBounds, transform_y};
use super::time_series::HyperChart;
use crate::tui::theme;

const X_AXIS_TICK_COUNT: usize = 4;
const MIN_HEIGHT_FOR_X_AXIS: u16 = 7;

pub(super) fn render(chart: &HyperChart<'_>, area: Rect, buf: &mut Buffer, gutter_width: u16) {
    let has_x_axis =
        !matches!(chart.x_axis_kind(), XAxis::Hidden) && area.height >= MIN_HEIGHT_FOR_X_AXIS;
    let rows = if has_x_axis {
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(1)]).split(area)
    };
    let chart_area = rows[0];
    let x_axis_area = has_x_axis.then(|| rows[1]);
    let layout = Layout::horizontal([Constraint::Length(gutter_width), Constraint::Min(1)])
        .split(chart_area);
    let gutter_area = layout[0];
    let scene = chart.scene();
    let bounds = scene.bounds;
    let caps = chart.caps();

    render_y_gutter(chart, gutter_area, layout[1], &scene, buf);

    // A graphics-protocol image owns every cell it covers: text written into
    // one of them breaks the terminal's placement of the rest of that row.
    // Pixel charts therefore leave markers to the raster and move value
    // labels into a rail beside the image.
    let pixels = cfg!(feature = "tui-graphics") && caps.graphics_protocol.is_pixels();
    let (plot_area, overlay) = if pixels {
        annotations::split_value_rail(layout[1])
    } else {
        (layout[1], annotations::Overlay::Inline)
    };

    if !render_plot(&scene, plot_area, buf, caps, pixels) {
        render_canvas_fallback(&scene, plot_area, buf, caps);
    }
    annotations::render(chart, &scene, plot_area, overlay, buf);
    if let Some(axis_area) = x_axis_area {
        render_x_axis(chart.x_axis_kind(), axis_area, plot_area, bounds, buf);
    }
}

/// Draw the plot with the richest renderer the terminal supports, returning
/// false when the caller should fall back to the cell canvas.
#[cfg_attr(not(feature = "tui-graphics"), allow(unused_variables))]
fn render_plot(
    scene: &ChartScene<'_>,
    plot_area: Rect,
    buf: &mut Buffer,
    caps: crate::tui::render_caps::RenderCaps,
    pixels: bool,
) -> bool {
    #[cfg(feature = "tui-graphics")]
    if pixels && super::pixel::render_scene(scene, plot_area, buf) {
        return true;
    }

    super::subcell::render(scene, plot_area, buf, caps)
}

fn render_canvas_fallback(
    scene: &ChartScene<'_>,
    plot_area: Rect,
    buf: &mut Buffer,
    caps: crate::tui::render_caps::RenderCaps,
) {
    let bounds = scene.bounds;
    let plot_density = (usize::from(plot_area.width.max(1)) * 4).max(160);
    let paths: Vec<Vec<Vec<(f64, f64)>>> = scene
        .series
        .iter()
        .map(|series| {
            series
                .data
                .visible_segments()
                .into_iter()
                .map(|segment| axis::interpolate_fill(&segment, plot_density))
                .collect()
        })
        .collect();

    let canvas = Canvas::default()
        .background_color(theme::bg_base())
        .marker(caps.glyph_tier.marker())
        .x_bounds([bounds.x_min, bounds.x_max])
        .y_bounds([bounds.y_min, bounds.y_max])
        .paint(|ctx| {
            draw_grid(ctx, scene);

            for (series, series_paths) in scene.series.iter().zip(paths.iter()) {
                let Some(bands) = series.fill.bands(caps, usize::from(plot_area.height)) else {
                    continue;
                };
                for path in series_paths {
                    for &(x, y) in path {
                        let y = scene.transform_y(series, y);
                        draw_gradient_column(ctx, x, y, &bands);
                    }
                }
            }

            ctx.layer();

            for (series, series_paths) in scene.series.iter().zip(paths.iter()) {
                for path in series_paths {
                    for pair in path.windows(2) {
                        let [(x1, y1), (x2, y2)] = pair else {
                            continue;
                        };
                        ctx.draw(&CanvasLine {
                            x1: *x1,
                            y1: scene.transform_y(series, *y1),
                            x2: *x2,
                            y2: scene.transform_y(series, *y2),
                            color: series.line_color,
                        });
                    }
                }
            }
        });

    canvas.render(plot_area, buf);
}

fn draw_grid(ctx: &mut ratatui::widgets::canvas::Context<'_>, scene: &ChartScene<'_>) {
    let grid_color = theme::border_unfocused();
    let baseline_color = theme::border_unfocused();

    for y in scene.gridlines() {
        ctx.draw(&CanvasLine {
            x1: scene.bounds.x_min,
            y1: y,
            x2: scene.bounds.x_max,
            y2: y,
            color: grid_color,
        });
    }

    ctx.draw(&CanvasLine {
        x1: scene.bounds.x_min,
        y1: 0.0,
        x2: scene.bounds.x_max,
        y2: 0.0,
        color: baseline_color,
    });
}

fn render_y_gutter(
    chart: &HyperChart<'_>,
    gutter_area: Rect,
    plot_area: Rect,
    scene: &ChartScene<'_>,
    buf: &mut Buffer,
) {
    match scene.baseline {
        Baseline::Zero { y_max } => {
            let labels = chart.build_y_labels(y_max);
            let divisions = chart.tick_count_value().saturating_sub(1).max(1);
            for (idx, label) in labels.iter().enumerate() {
                let y = y_max * fraction(idx, divisions);
                let row = y_to_row(plot_area, scene.bounds, y);
                Paragraph::new(ratatui::text::Line::from(label.clone())).render(
                    Rect {
                        x: gutter_area.x,
                        y: row,
                        width: gutter_area.width,
                        height: 1,
                    },
                    buf,
                );
            }
        }
        Baseline::Mirror {
            upper_max,
            lower_max,
            upper_label,
            lower_label,
        } => {
            // Lane names hug the baseline and claim their rows first; tick
            // labels then fill the remaining rows from each lane's outer
            // edge inward, so a short lane drops its inner ticks instead of
            // printing two labels on one row.
            let baseline_row = y_to_row(plot_area, scene.bounds, 0.0);
            let label_style = Style::default().fg(theme::border_unfocused());
            let mut occupied = vec![baseline_row];
            if baseline_row > plot_area.y {
                let row = baseline_row - 1;
                render_text(buf, gutter_row(gutter_area, row), upper_label, label_style);
                occupied.push(row);
            }
            if baseline_row + 1 < plot_area.y + plot_area.height {
                let row = baseline_row + 1;
                render_text(buf, gutter_row(gutter_area, row), lower_label, label_style);
                occupied.push(row);
            }

            let divisions = chart.tick_count_value().saturating_sub(1).max(1);
            for (max_value, direction) in [
                (upper_max, SeriesDirection::Up),
                (lower_max, SeriesDirection::Down),
            ] {
                let labels = chart.build_y_labels(max_value);
                for (idx, label) in labels.iter().enumerate().skip(1).rev() {
                    let value = max_value * fraction(idx, divisions);
                    let y = transform_y(scene.baseline, direction, value);
                    let row = y_to_row(plot_area, scene.bounds, y);
                    if occupied.contains(&row) {
                        continue;
                    }
                    occupied.push(row);
                    Paragraph::new(ratatui::text::Line::from(label.clone()))
                        .render(gutter_row(gutter_area, row), buf);
                }
            }
        }
    }
}

fn gutter_row(gutter_area: Rect, row: u16) -> Rect {
    Rect {
        x: gutter_area.x,
        y: row,
        width: gutter_area.width,
        height: 1,
    }
}

fn render_x_axis(
    x_axis: XAxis,
    axis_area: Rect,
    plot_area: Rect,
    bounds: PlotBounds,
    buf: &mut Buffer,
) {
    let axis_style = Style::default().fg(theme::border_unfocused());
    let mut occupied_until = axis_area.x;
    for (idx, (x, label)) in x_labels(x_axis, bounds).iter().enumerate() {
        let column = x_to_col(plot_area, bounds, *x);
        let width = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
        let mut start = column.saturating_sub(width / 2);
        if idx + 1 == X_AXIS_TICK_COUNT {
            start = column.saturating_sub(width.saturating_sub(1));
        }
        start = start.max(plot_area.x);
        let end = start
            .saturating_add(width)
            .min(plot_area.x + plot_area.width);
        if end <= occupied_until || start >= plot_area.x + plot_area.width {
            continue;
        }

        render_text(
            buf,
            Rect {
                x: start,
                y: axis_area.y,
                width: end - start,
                height: 1,
            },
            label,
            axis_style,
        );
        occupied_until = end.saturating_add(1);
    }
}

fn x_labels(x_axis: XAxis, bounds: PlotBounds) -> Vec<(f64, String)> {
    let divisions = X_AXIS_TICK_COUNT.saturating_sub(1).max(1);
    (0..X_AXIS_TICK_COUNT)
        .map(|idx| {
            let t = fraction(idx, divisions);
            let x = bounds.x_min + (bounds.x_max - bounds.x_min) * t;
            (
                x,
                format_x_label(x_axis, x, bounds.x_max, idx + 1 == X_AXIS_TICK_COUNT),
            )
        })
        .collect()
}

fn format_x_label(x_axis: XAxis, x: f64, x_max: f64, is_last: bool) -> String {
    match x_axis {
        XAxis::Hidden => String::new(),
        XAxis::Relative { sample_interval } => {
            if is_last {
                return "now".into();
            }
            let seconds = ((x_max - x) * sample_interval.as_secs_f64()).round();
            format_relative_offset(seconds)
        }
        XAxis::Epoch => {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            let epoch = x.round() as i64;
            DateTime::<Utc>::from_timestamp(epoch, 0).map_or_else(
                || format!("{x:.0}"),
                |time| time.format("%H:%M").to_string(),
            )
        }
    }
}

fn draw_gradient_column(
    ctx: &mut ratatui::widgets::canvas::Context<'_>,
    x: f64,
    y: f64,
    bands: &[ratatui::style::Color],
) {
    if y.abs() < f64::EPSILON || bands.is_empty() {
        return;
    }

    let divisions = bands.len();
    for (idx, color) in bands.iter().enumerate() {
        let start = fraction(idx, divisions);
        let end = fraction(idx + 1, divisions);
        ctx.draw(&CanvasLine {
            x1: x,
            y1: y * start,
            x2: x,
            y2: y * end,
            color: *color,
        });
    }
}

fn format_relative_offset(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round();
    if seconds < 60.0 {
        format!("-{seconds:.0}s")
    } else if seconds < 3_600.0 {
        format!("-{:.0}m", seconds / 60.0)
    } else {
        format!("-{:.0}h", seconds / 3_600.0)
    }
}
