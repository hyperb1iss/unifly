//! Graphics-protocol pixel renderer integration for HyperChart.

use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;

use super::model::{Baseline, FillStyle, SeriesData, SeriesDirection, XAxis};
use super::owned_scene::OwnedChartScene;
use super::scene::{AnnotationKind, ChartScene, PlotBounds};
use crate::tui::render_caps;

pub(super) fn render_scene(scene: &ChartScene<'_>, area: Rect, buf: &mut Buffer) -> bool {
    use crate::tui::graphics::CachedChart;

    let Some(picker) = crate::tui::graphics::current_picker() else {
        return false;
    };
    let font_size = picker.font_size();
    let target = Size::new(area.width.max(1), area.height.max(1));
    let slot = graphics_chart_slot_key(scene, area, font_size);
    let key = graphics_chart_key(scene, area, font_size);

    match crate::tui::graphics::render_cached_chart(slot, key, area, buf) {
        CachedChart::Rendered => return true,
        CachedChart::Stale(current) => {
            if matches!(current, crate::tui::graphics::CachedChartStatus::Missing) {
                queue_graphics_scene(slot, key, scene, area, target, font_size);
            }
            return true;
        }
        CachedChart::Pending | CachedChart::Failed => return false,
        CachedChart::Missing => {}
    }

    queue_graphics_scene(slot, key, scene, area, target, font_size);
    false
}

fn queue_graphics_scene(
    slot: crate::tui::graphics::ChartSlotKey,
    key: crate::tui::graphics::ChartImageKey,
    scene: &ChartScene<'_>,
    area: Rect,
    target: Size,
    font_size: ratatui_image::FontSize,
) {
    use image::DynamicImage;

    use super::raster::rasterize_scene;

    let raster_size = super::raster::RasterSize {
        width: u32::from(area.width.max(1)).saturating_mul(u32::from(font_size.width.max(1))),
        height: u32::from(area.height.max(1)).saturating_mul(u32::from(font_size.height.max(1))),
    };
    // Snapshot the scene into owned data so rasterization runs on the
    // graphics worker thread instead of blocking the render loop.
    let owned = OwnedChartScene::capture(scene);
    crate::tui::graphics::queue_chart(slot, key, target, move || {
        DynamicImage::ImageRgba8(rasterize_scene(&owned.as_scene(), raster_size))
    });
}

fn graphics_chart_slot_key(
    scene: &ChartScene<'_>,
    area: Rect,
    font_size: ratatui_image::FontSize,
) -> crate::tui::graphics::ChartSlotKey {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut state = DefaultHasher::new();
    area.width.hash(&mut state);
    area.height.hash(&mut state);
    font_size.width.hash(&mut state);
    font_size.height.hash(&mut state);
    render_caps::current().graphics_protocol.hash(&mut state);
    hash_x_axis(scene.x_axis, &mut state);
    hash_baseline_shape(scene.baseline, &mut state);
    scene.grid.tick_count.hash(&mut state);
    for series in &scene.series {
        series.name.hash(&mut state);
        hash_color(series.line_color, &mut state);
        hash_fill(series.fill, &mut state);
        match series.direction {
            SeriesDirection::Up => 1u8.hash(&mut state),
            SeriesDirection::Down => 2u8.hash(&mut state),
        }
    }
    crate::tui::graphics::ChartSlotKey(state.finish())
}

fn graphics_chart_key(
    scene: &ChartScene<'_>,
    area: Rect,
    font_size: ratatui_image::FontSize,
) -> crate::tui::graphics::ChartImageKey {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut state = DefaultHasher::new();
    area.width.hash(&mut state);
    area.height.hash(&mut state);
    font_size.width.hash(&mut state);
    font_size.height.hash(&mut state);
    render_caps::current().graphics_protocol.hash(&mut state);
    hash_x_axis(scene.x_axis, &mut state);
    hash_bounds(scene.bounds, &mut state);
    hash_baseline(scene.baseline, &mut state);
    scene.grid.tick_count.hash(&mut state);
    for series in &scene.series {
        series.name.hash(&mut state);
        hash_series_data(series.data, &mut state);
        hash_color(series.line_color, &mut state);
        hash_fill(series.fill, &mut state);
        match series.direction {
            SeriesDirection::Up => 1u8.hash(&mut state),
            SeriesDirection::Down => 2u8.hash(&mut state),
        }
    }
    for annotation in &scene.annotations {
        match annotation.kind {
            AnnotationKind::Now => 1u8.hash(&mut state),
            AnnotationKind::Peak => 2u8.hash(&mut state),
        }
        hash_f64(annotation.x, &mut state);
        hash_f64(annotation.y, &mut state);
        hash_f64(annotation.transformed_y, &mut state);
        hash_color(annotation.color, &mut state);
    }
    crate::tui::graphics::ChartImageKey(state.finish())
}

fn hash_x_axis(axis: XAxis, state: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    match axis {
        XAxis::Hidden => 0u8.hash(state),
        XAxis::Relative { sample_interval } => {
            1u8.hash(state);
            sample_interval.as_nanos().hash(state);
        }
        XAxis::Epoch => 2u8.hash(state),
    }
}

fn hash_bounds(bounds: PlotBounds, state: &mut impl std::hash::Hasher) {
    hash_f64(bounds.x_min, state);
    hash_f64(bounds.x_max, state);
    hash_f64(bounds.y_min, state);
    hash_f64(bounds.y_max, state);
}

fn hash_baseline(baseline: Baseline<'_>, state: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    match baseline {
        Baseline::Zero { y_max } => {
            0u8.hash(state);
            hash_f64(y_max, state);
        }
        Baseline::Mirror {
            upper_max,
            lower_max,
            upper_label,
            lower_label,
        } => {
            1u8.hash(state);
            hash_f64(upper_max, state);
            hash_f64(lower_max, state);
            upper_label.hash(state);
            lower_label.hash(state);
        }
    }
}

fn hash_baseline_shape(baseline: Baseline<'_>, state: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    match baseline {
        Baseline::Zero { .. } => 0u8.hash(state),
        Baseline::Mirror {
            upper_label,
            lower_label,
            ..
        } => {
            1u8.hash(state);
            upper_label.hash(state);
            lower_label.hash(state);
        }
    }
}

fn hash_series_data(data: SeriesData<'_>, state: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    match data {
        SeriesData::Dense(points) => {
            0u8.hash(state);
            points.len().hash(state);
            for (x, y) in points {
                hash_f64(*x, state);
                hash_f64(*y, state);
            }
        }
        SeriesData::Gapped(points) => {
            1u8.hash(state);
            points.len().hash(state);
            for point in points {
                hash_f64(point.x, state);
                match point.y {
                    Some(y) => {
                        1u8.hash(state);
                        hash_f64(y, state);
                    }
                    None => 0u8.hash(state),
                }
            }
        }
    }
}

fn hash_fill(fill: FillStyle, state: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    match fill {
        FillStyle::None => 0u8.hash(state),
        FillStyle::Solid(color) => {
            1u8.hash(state);
            hash_color(color, state);
        }
        FillStyle::Gradient(gradient) => {
            2u8.hash(state);
            let (start, end) = gradient.endpoints();
            hash_color(start, state);
            hash_color(end, state);
        }
    }
}

fn hash_color(color: Color, state: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    match color {
        Color::Reset => 0u8.hash(state),
        Color::Black => 1u8.hash(state),
        Color::Red => 2u8.hash(state),
        Color::Green => 3u8.hash(state),
        Color::Yellow => 4u8.hash(state),
        Color::Blue => 5u8.hash(state),
        Color::Magenta => 6u8.hash(state),
        Color::Cyan => 7u8.hash(state),
        Color::Gray => 8u8.hash(state),
        Color::DarkGray => 9u8.hash(state),
        Color::LightRed => 10u8.hash(state),
        Color::LightGreen => 11u8.hash(state),
        Color::LightYellow => 12u8.hash(state),
        Color::LightBlue => 13u8.hash(state),
        Color::LightMagenta => 14u8.hash(state),
        Color::LightCyan => 15u8.hash(state),
        Color::White => 16u8.hash(state),
        Color::Rgb(r, g, b) => {
            17u8.hash(state);
            r.hash(state);
            g.hash(state);
            b.hash(state);
        }
        Color::Indexed(index) => {
            18u8.hash(state);
            index.hash(state);
        }
    }
}

fn hash_f64(value: f64, state: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    value.to_bits().hash(state);
}
