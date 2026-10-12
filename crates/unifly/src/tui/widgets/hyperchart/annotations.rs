//! Marker and value label overlay rendering for HyperChart.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::geometry::{render_text, x_to_col, y_to_row};
use super::model::SeriesDirection;
use super::scene::{Annotation, AnnotationKind, ChartScene, PlotBounds};
use super::time_series::HyperChart;

const MARKER_VALUE_WIDTH: u16 = 9;
const MIN_PLOT_WIDTH_FOR_RAIL: u16 = MARKER_VALUE_WIDTH * 4;

/// Where a chart's markers and value labels are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Overlay {
    /// Text markers and value labels inside the plot.
    Inline,
    /// Value labels in a rail right of the plot; the raster draws markers.
    Rail(Rect),
    /// Raster markers only, for a plot too narrow to spare a rail.
    RasterOnly,
}

/// Carve a value-label rail off the right of a pixel chart's plot area.
pub(super) fn split_value_rail(area: Rect) -> (Rect, Overlay) {
    if area.width < MIN_PLOT_WIDTH_FOR_RAIL {
        return (area, Overlay::RasterOnly);
    }
    let plot = Rect {
        width: area.width - MARKER_VALUE_WIDTH,
        ..area
    };
    let rail = Rect {
        x: plot.x + plot.width,
        width: MARKER_VALUE_WIDTH,
        ..area
    };
    (plot, Overlay::Rail(rail))
}

pub(super) fn render(
    chart: &HyperChart<'_>,
    scene: &ChartScene<'_>,
    plot_area: Rect,
    overlay: Overlay,
    buf: &mut Buffer,
) {
    match overlay {
        Overlay::Inline => render_inline(chart, scene, plot_area, buf),
        Overlay::Rail(rail) => render_rail(chart, scene, plot_area, rail, buf),
        Overlay::RasterOnly => {}
    }
}

/// Value labels for the newest points, one per row, beside the plot.
fn render_rail(
    chart: &HyperChart<'_>,
    scene: &ChartScene<'_>,
    plot_area: Rect,
    rail: Rect,
    buf: &mut Buffer,
) {
    let mut occupied: Vec<u16> = Vec::new();
    let now_points = scene
        .annotations
        .iter()
        .filter(|annotation| annotation.kind == AnnotationKind::Now);
    for annotation in now_points {
        let Some(row) = free_rail_row(
            y_to_row(plot_area, scene.bounds, annotation.transformed_y),
            matches!(annotation.direction, SeriesDirection::Up),
            rail,
            &occupied,
        ) else {
            continue;
        };
        occupied.push(row);
        render_text(
            buf,
            Rect {
                x: rail.x,
                y: row,
                width: rail.width,
                height: 1,
            },
            &format!("─ {}", chart.format_value(annotation.y)),
            Style::default().fg(annotation.color),
        );
    }
}

/// The marker's own row, or the nearest free row away from the baseline when
/// another label already sits there (both lanes idle at zero, say). Upward
/// series move up and downward series move down, staying in their lanes.
fn free_rail_row(row: u16, upward: bool, rail: Rect, occupied: &[u16]) -> Option<u16> {
    let mut row = row;
    while occupied.contains(&row) {
        row = if upward {
            row.checked_sub(1).filter(|&next| next >= rail.y)?
        } else {
            Some(row + 1).filter(|&next| next < rail.y + rail.height)?
        };
    }
    Some(row)
}

fn render_inline(
    chart: &HyperChart<'_>,
    scene: &ChartScene<'_>,
    plot_area: Rect,
    buf: &mut Buffer,
) {
    for annotation in &scene.annotations {
        let symbol = match annotation.kind {
            AnnotationKind::Now => "●",
            AnnotationKind::Peak => "◆",
        };
        render_point_marker(symbol, *annotation, plot_area, scene.bounds, buf);
        if annotation.kind == AnnotationKind::Now {
            render_marker_value(chart, *annotation, plot_area, scene.bounds, buf);
        }
    }
}

fn render_point_marker(
    symbol: &str,
    point: Annotation,
    plot_area: Rect,
    bounds: PlotBounds,
    buf: &mut Buffer,
) {
    let column = x_to_col(plot_area, bounds, point.x);
    let row = y_to_row(plot_area, bounds, point.transformed_y);
    render_text(
        buf,
        Rect {
            x: column,
            y: row,
            width: 1,
            height: 1,
        },
        symbol,
        Style::default().fg(point.color),
    );
}

fn render_marker_value(
    chart: &HyperChart<'_>,
    point: Annotation,
    plot_area: Rect,
    bounds: PlotBounds,
    buf: &mut Buffer,
) {
    let column = x_to_col(plot_area, bounds, point.x);
    let row = y_to_row(plot_area, bounds, point.transformed_y);
    let label = format!("─ {}", chart.format_value(point.y));
    let style = Style::default().fg(point.color);
    let right_x = column.saturating_add(1);
    if right_x + MARKER_VALUE_WIDTH <= plot_area.x + plot_area.width {
        render_text(
            buf,
            Rect {
                x: right_x,
                y: row,
                width: MARKER_VALUE_WIDTH,
                height: 1,
            },
            &label,
            style,
        );
        return;
    }

    if column >= plot_area.x + MARKER_VALUE_WIDTH {
        render_text(
            buf,
            Rect {
                x: column - MARKER_VALUE_WIDTH,
                y: row,
                width: MARKER_VALUE_WIDTH,
                height: 1,
            },
            &label,
            style,
        );
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::text::Line;

    use super::super::scene::GridSpec;
    use super::*;
    use crate::tui::widgets::hyperchart::{Baseline, Domain, Series, XAxis};

    fn buffer_text(buf: &Buffer) -> String {
        (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .filter_map(|x| buf.cell((x, y)).map(|cell| cell.symbol().to_string()))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn now_annotation_renders_value_when_there_is_room_on_the_right() {
        let series: [Series<'_>; 0] = [];
        let chart = HyperChart::new(Line::raw(""), &series, (0.0, 10.0), 1_024.0)
            .domain(Domain::Rate)
            .baseline(Baseline::Zero { y_max: 1_024.0 });
        let plot_area = Rect::new(0, 0, 24, 4);
        let mut buf = Buffer::empty(plot_area);
        let scene = ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max: 10.0,
                y_min: 0.0,
                y_max: 1_024.0,
            },
            baseline: Baseline::Zero { y_max: 1_024.0 },
            series: Vec::new(),
            grid: GridSpec { tick_count: 4 },
            annotations: vec![Annotation {
                kind: AnnotationKind::Now,
                x: 2.0,
                y: 1_024.0,
                transformed_y: 1_024.0,
                color: Color::Cyan,
                direction: SeriesDirection::Up,
            }],
        };

        render(&chart, &scene, plot_area, Overlay::Inline, &mut buf);

        let text = buffer_text(&buf);
        assert!(text.contains("●"));
        assert!(text.contains("─"));
    }

    fn now_scene(annotations: Vec<Annotation>) -> ChartScene<'static> {
        ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max: 10.0,
                y_min: 0.0,
                y_max: 1_024.0,
            },
            baseline: Baseline::Zero { y_max: 1_024.0 },
            series: Vec::new(),
            grid: GridSpec { tick_count: 4 },
            annotations,
        }
    }

    fn now_at(y: f64, color: Color) -> Annotation {
        Annotation {
            kind: AnnotationKind::Now,
            x: 10.0,
            y,
            transformed_y: y,
            color,
            direction: SeriesDirection::Up,
        }
    }

    #[test]
    fn rail_overlay_keeps_every_plot_cell_untouched() {
        let series: [Series<'_>; 0] = [];
        let chart = HyperChart::new(Line::raw(""), &series, (0.0, 10.0), 1_024.0)
            .domain(Domain::Rate)
            .baseline(Baseline::Zero { y_max: 1_024.0 });
        let area = Rect::new(0, 0, 48, 6);
        let mut buf = Buffer::empty(area);
        let (plot_area, overlay) = split_value_rail(area);
        let Overlay::Rail(rail) = overlay else {
            panic!("a 48-column plot should get a rail");
        };
        let mut scene = now_scene(vec![now_at(512.0, Color::Cyan)]);
        scene.annotations.push(Annotation {
            kind: AnnotationKind::Peak,
            ..now_at(1_024.0, Color::Magenta)
        });

        render(&chart, &scene, plot_area, overlay, &mut buf);

        for y in plot_area.top()..plot_area.bottom() {
            for x in plot_area.left()..plot_area.right() {
                assert_eq!(buf[(x, y)].symbol(), " ", "plot cell {x},{y} was written");
            }
        }
        let rail_text: String = (rail.left()..rail.right())
            .flat_map(|x| (rail.top()..rail.bottom()).map(move |y| (x, y)))
            .map(|cell| buf[cell].symbol().to_string())
            .collect();
        assert!(rail_text.contains('─'));
    }

    #[test]
    fn idle_lanes_keep_their_rail_labels_on_their_own_side() {
        let series: [Series<'_>; 0] = [];
        let chart = HyperChart::new(Line::raw(""), &series, (0.0, 10.0), 1_024.0)
            .domain(Domain::Rate)
            .baseline(Baseline::Zero { y_max: 1_024.0 });
        let area = Rect::new(0, 0, 48, 9);
        let mut buf = Buffer::empty(area);
        let (plot_area, overlay) = split_value_rail(area);
        let mut scene = now_scene(vec![
            now_at(0.0, Color::Magenta),
            Annotation {
                transformed_y: -0.0,
                direction: SeriesDirection::Down,
                ..now_at(0.0, Color::Cyan)
            },
        ]);
        scene.bounds.y_min = -512.0;

        render(&chart, &scene, plot_area, overlay, &mut buf);

        let row_of = |color: Color| {
            (0..area.height).find(|&y| {
                let cell = &buf[(plot_area.right(), y)];
                cell.symbol() == "─" && cell.fg == color
            })
        };
        let rx = row_of(Color::Magenta).expect("RX label");
        let tx = row_of(Color::Cyan).expect("TX label");
        assert!(tx > rx, "TX label at row {tx} should sit below RX at {rx}");
    }

    #[test]
    fn narrow_pixel_plots_skip_the_rail() {
        let area = Rect::new(0, 0, MIN_PLOT_WIDTH_FOR_RAIL - 1, 6);

        assert_eq!(split_value_rail(area), (area, Overlay::RasterOnly));
    }

    #[test]
    fn rail_labels_on_one_row_move_apart() {
        let series: [Series<'_>; 0] = [];
        let chart = HyperChart::new(Line::raw(""), &series, (0.0, 10.0), 1_024.0)
            .domain(Domain::Rate)
            .baseline(Baseline::Zero { y_max: 1_024.0 });
        let area = Rect::new(0, 0, 48, 6);
        let mut buf = Buffer::empty(area);
        let (plot_area, overlay) = split_value_rail(area);
        let scene = now_scene(vec![
            now_at(512.0, Color::Cyan),
            now_at(512.0, Color::Magenta),
        ]);

        render(&chart, &scene, plot_area, overlay, &mut buf);

        let labelled_rows = (0..area.height)
            .filter(|&y| buf[(plot_area.right(), y)].symbol() == "─")
            .count();
        assert_eq!(labelled_rows, 2);
    }
}
