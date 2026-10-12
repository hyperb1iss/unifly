//! tiny-skia rasterizer for graphics-protocol chart rendering.
//!
//! Renders a [`ChartScene`] into an RGBA image with anti-aliased strokes,
//! monotone cubic interpolation between samples, vertical gradient fills
//! that fade toward the baseline, and a glow understroke beneath a crisp
//! core line. Sample runs denser than the pixel grid are reduced with M4
//! (first/min/max/last per column) so traffic spikes never alias away.

use image::RgbaImage;
use ratatui::style::Color;
use tiny_skia::{
    FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pixmap,
    Point, SpreadMode, Stroke, Transform,
};

use super::color::color_to_rgb;
use super::curve;
use super::model::{FillStyle, SeriesDirection};
use super::scene::{AnnotationKind, ChartScene, PlotBounds, SceneSeries};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterSize {
    pub width: u32,
    pub height: u32,
}

const FILL_ALPHA_AT_LINE: u8 = 148;
const FILL_ALPHA_AT_BASELINE: u8 = 10;
const GLOW_ALPHA: u8 = 54;
const CORE_ALPHA: u8 = 242;
const GRID_ALPHA: u8 = 40;
const BASELINE_ALPHA: u8 = 84;

#[derive(Debug, Clone, Copy)]
struct Frame {
    bounds: PlotBounds,
    width: u32,
    height: u32,
}

impl Frame {
    #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
    fn x_px(self, x: f64) -> f32 {
        let span = (self.bounds.x_max - self.bounds.x_min).max(f64::EPSILON);
        let ratio = ((x - self.bounds.x_min) / span).clamp(0.0, 1.0);
        (ratio * f64::from(self.width.saturating_sub(1))) as f32
    }

    #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
    fn y_px(self, y: f64) -> f32 {
        let span = (self.bounds.y_max - self.bounds.y_min).max(f64::EPSILON);
        let ratio = ((self.bounds.y_max - y) / span).clamp(0.0, 1.0);
        (ratio * f64::from(self.height.saturating_sub(1))) as f32
    }

    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    fn core_width(self) -> f32 {
        (self.height as f32 / 110.0).clamp(1.4, 2.6)
    }

    fn glow_width(self) -> f32 {
        self.core_width() * 3.2
    }

    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    fn right_edge(self) -> f32 {
        self.width.saturating_sub(1) as f32
    }

    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    fn bottom_edge(self) -> f32 {
        self.height.saturating_sub(1) as f32
    }
}

pub fn rasterize_scene(scene: &ChartScene<'_>, size: RasterSize) -> RgbaImage {
    let width = size.width.max(1);
    let height = size.height.max(1);
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        return RgbaImage::new(width, height);
    };
    let frame = Frame {
        bounds: scene.bounds,
        width,
        height,
    };

    draw_grid(&mut pixmap, scene, frame);
    for series in &scene.series {
        draw_series(&mut pixmap, scene, series, frame);
    }
    draw_annotations(&mut pixmap, scene, frame);

    to_rgba_image(&pixmap)
}

fn draw_grid(pixmap: &mut Pixmap, scene: &ChartScene<'_>, frame: Frame) {
    let mut builder = PathBuilder::new();
    for y in scene.gridlines() {
        let row = frame.y_px(y);
        builder.move_to(0.0, row);
        builder.line_to(frame.right_edge(), row);
    }
    if let Some(path) = builder.finish() {
        stroke_path(pixmap, &path, Color::DarkGray, GRID_ALPHA, 1.0);
    }

    let baseline = frame.y_px(0.0);
    let mut builder = PathBuilder::new();
    builder.move_to(0.0, baseline);
    builder.line_to(frame.right_edge(), baseline);
    if let Some(path) = builder.finish() {
        stroke_path(pixmap, &path, Color::Gray, BASELINE_ALPHA, 1.0);
    }
}

fn draw_series(
    pixmap: &mut Pixmap,
    scene: &ChartScene<'_>,
    series: &SceneSeries<'_>,
    frame: Frame,
) {
    for segment in series.data.visible_segments() {
        let points = pixel_points(scene, series, &segment, frame);
        if points.len() < 2 {
            continue;
        }

        if let Some(path) = curve::fill_path(&points, frame.y_px(0.0))
            && let Some(paint) = fill_paint(series.fill, series.direction, frame)
        {
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        if let Some(path) = curve::curve_path(&points) {
            stroke_path(
                pixmap,
                &path,
                series.line_color,
                GLOW_ALPHA,
                frame.glow_width(),
            );
            stroke_path(
                pixmap,
                &path,
                series.line_color,
                CORE_ALPHA,
                frame.core_width(),
            );
        }
    }
}

fn draw_annotations(pixmap: &mut Pixmap, scene: &ChartScene<'_>, frame: Frame) {
    for annotation in &scene.annotations {
        let x = frame.x_px(annotation.x);
        let y = frame.y_px(annotation.transformed_y);
        let radius = frame.core_width() * 1.6;
        match annotation.kind {
            AnnotationKind::Now => {
                if let Some(halo) = PathBuilder::from_circle(x, y, radius * 2.4) {
                    fill_solid(pixmap, &halo, annotation.color, GLOW_ALPHA);
                }
                if let Some(disc) = PathBuilder::from_circle(x, y, radius) {
                    fill_solid(pixmap, &disc, annotation.color, 255);
                }
            }
            AnnotationKind::Peak => {
                let reach = radius * 1.8;
                let mut builder = PathBuilder::new();
                builder.move_to(x, y - reach);
                builder.line_to(x + reach, y);
                builder.line_to(x, y + reach);
                builder.line_to(x - reach, y);
                builder.close();
                if let Some(diamond) = builder.finish() {
                    fill_solid(pixmap, &diamond, annotation.color, 230);
                }
            }
        }
    }
}

/// Map a data segment into pixel space, reducing with M4 when the segment
/// is denser than the pixel grid.
fn pixel_points(
    scene: &ChartScene<'_>,
    series: &SceneSeries<'_>,
    segment: &[(f64, f64)],
    frame: Frame,
) -> Vec<(f32, f32)> {
    let points: Vec<(f32, f32)> = segment
        .iter()
        .map(|&(x, y)| (frame.x_px(x), frame.y_px(scene.transform_y(series, y))))
        .collect();
    curve::downsample_m4(points, frame.width)
}

fn fill_paint<'a>(fill: FillStyle, direction: SeriesDirection, frame: Frame) -> Option<Paint<'a>> {
    let (base_color, line_color) = match fill {
        FillStyle::None => return None,
        FillStyle::Solid(color) => (color, color),
        FillStyle::Gradient(gradient) => gradient.endpoints(),
    };

    let baseline = frame.y_px(0.0);
    let extreme = match direction {
        SeriesDirection::Up => 0.0,
        SeriesDirection::Down => frame.bottom_edge(),
    };

    let shader = LinearGradient::new(
        Point::from_xy(0.0, extreme),
        Point::from_xy(0.0, baseline),
        vec![
            GradientStop::new(0.0, skia_color(line_color, FILL_ALPHA_AT_LINE)),
            GradientStop::new(1.0, skia_color(base_color, FILL_ALPHA_AT_BASELINE)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or_else(|| {
        tiny_skia::Shader::SolidColor(skia_color(line_color, FILL_ALPHA_AT_LINE / 2))
    });
    Some(Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    })
}

fn stroke_path(pixmap: &mut Pixmap, path: &Path, color: Color, alpha: u8, width: f32) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(skia_color(color, alpha));
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), None);
}

fn fill_solid(pixmap: &mut Pixmap, path: &Path, color: Color, alpha: u8) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(skia_color(color, alpha));
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn skia_color(color: Color, alpha: u8) -> tiny_skia::Color {
    let (red, green, blue) = color_to_rgb(color);
    tiny_skia::Color::from_rgba8(red, green, blue, alpha)
}

fn to_rgba_image(pixmap: &Pixmap) -> RgbaImage {
    let mut data = Vec::with_capacity(pixmap.pixels().len() * 4);
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        data.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    RgbaImage::from_raw(pixmap.width(), pixmap.height(), data)
        .unwrap_or_else(|| RgbaImage::new(pixmap.width(), pixmap.height()))
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;
    use crate::tui::widgets::hyperchart::model::{
        Baseline, FillStyle, SeriesData, SeriesDirection, XAxis,
    };
    use crate::tui::widgets::hyperchart::scene::{Annotation, GridSpec};

    fn scene(series: Vec<SceneSeries<'_>>, y_max: f64, x_max: f64) -> ChartScene<'_> {
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

    #[test]
    fn rasterizer_draws_non_empty_scene() {
        let data = [(0.0, 1.0), (1.0, 3.0), (2.0, 2.0)];
        let mut chart_scene = scene(
            vec![SceneSeries {
                name: "rx",
                data: SeriesData::Dense(&data),
                line_color: Color::Cyan,
                fill: FillStyle::Solid(Color::Blue),
                direction: SeriesDirection::Up,
            }],
            4.0,
            2.0,
        );
        chart_scene.annotations = vec![Annotation {
            kind: AnnotationKind::Now,
            x: 2.0,
            y: 2.0,
            transformed_y: 2.0,
            color: Color::Cyan,
            direction: SeriesDirection::Up,
        }];

        let image = rasterize_scene(
            &chart_scene,
            RasterSize {
                width: 80,
                height: 32,
            },
        );

        assert!(image.pixels().any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn fill_fades_toward_baseline() {
        let data = [(0.0, 2.0), (10.0, 2.0)];
        let chart_scene = scene(
            vec![SceneSeries {
                name: "rx",
                data: SeriesData::Dense(&data),
                line_color: Color::Cyan,
                fill: FillStyle::Gradient(super::super::ChartGradient::new(
                    Color::Black,
                    Color::Blue,
                )),
                direction: SeriesDirection::Up,
            }],
            4.0,
            10.0,
        );

        let image = rasterize_scene(
            &chart_scene,
            RasterSize {
                width: 40,
                height: 40,
            },
        );

        // Line sits at row ~19 of 40. Sample below the glow reach and just
        // above the baseline: the fill must fade on the way down.
        let near_line = image.get_pixel(20, 27)[3];
        let near_baseline = image.get_pixel(20, 37)[3];
        assert!(near_line > near_baseline, "{near_line} <= {near_baseline}");
        assert!(near_baseline > 0);
    }

    #[test]
    fn stroke_lands_on_the_line_row() {
        let data = [(0.0, 2.0), (10.0, 2.0)];
        let chart_scene = scene(
            vec![SceneSeries {
                name: "rx",
                data: SeriesData::Dense(&data),
                line_color: Color::Cyan,
                fill: FillStyle::None,
                direction: SeriesDirection::Up,
            }],
            4.0,
            10.0,
        );

        let image = rasterize_scene(
            &chart_scene,
            RasterSize {
                width: 40,
                height: 40,
            },
        );

        let on_line = image.get_pixel(20, 19)[3].max(image.get_pixel(20, 20)[3]);
        let far_away = image.get_pixel(20, 5)[3];
        assert!(on_line > 150, "core stroke missing: alpha {on_line}");
        assert_eq!(far_away, 0, "stray paint far from the line");
    }
}
