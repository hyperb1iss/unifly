//! Shared scene model for cell-native HyperChart renderers.

use ratatui::style::Color;

use super::model::{Baseline, FillStyle, SeriesData, SeriesDirection, XAxis};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotBounds {
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSpec {
    pub tick_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct SceneSeries<'a> {
    pub name: &'a str,
    pub data: SeriesData<'a>,
    pub line_color: Color,
    pub fill: FillStyle,
    pub direction: SeriesDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationKind {
    Now,
    Peak,
}

#[derive(Debug, Clone, Copy)]
pub struct Annotation {
    pub kind: AnnotationKind,
    pub x: f64,
    pub y: f64,
    pub transformed_y: f64,
    pub color: Color,
    pub direction: SeriesDirection,
}

#[derive(Debug, Clone)]
pub struct ChartScene<'a> {
    pub x_axis: XAxis,
    pub bounds: PlotBounds,
    pub baseline: Baseline<'a>,
    pub series: Vec<SceneSeries<'a>>,
    pub grid: GridSpec,
    pub annotations: Vec<Annotation>,
}

impl ChartScene<'_> {
    pub fn is_empty(&self) -> bool {
        self.series.iter().all(SceneSeries::is_empty)
    }

    pub fn transform_y(&self, series: &SceneSeries<'_>, value: f64) -> f64 {
        transform_y(self.baseline, series.direction, value)
    }

    pub fn gridlines(&self) -> Vec<f64> {
        let divisions = self.grid.tick_count.saturating_sub(1).max(1);
        let mut values = Vec::new();
        match self.baseline {
            Baseline::Zero { y_max } => {
                for idx in 1..=divisions {
                    values.push(y_max * fraction(idx, divisions));
                }
            }
            Baseline::Mirror { upper_max, .. } => {
                let lower_extent = mirror_lower_extent(upper_max);
                for idx in 1..=divisions {
                    let position = fraction(idx, divisions);
                    values.push(upper_max * position);
                    values.push(-lower_extent * position);
                }
            }
        }
        values
    }
}

/// Height of a mirror chart's lower lane relative to its upper lane.
const MIRROR_LOWER_LANE_SHARE: f64 = 0.5;

/// Plot-space depth of a mirror chart's lower lane, which spans
/// `[-extent, 0]` while the upper lane keeps its own units.
///
/// The lanes hold fixed shares of the plot height, so each keeps its own
/// scale without a large upper range squeezing the lower lane to one row.
pub(super) fn mirror_lower_extent(upper_max: f64) -> f64 {
    upper_max.max(1.0) * MIRROR_LOWER_LANE_SHARE
}

pub(super) fn transform_y(baseline: Baseline<'_>, direction: SeriesDirection, value: f64) -> f64 {
    match (baseline, direction) {
        (
            Baseline::Mirror {
                upper_max,
                lower_max,
                ..
            },
            SeriesDirection::Down,
        ) => {
            let fraction = if lower_max > 0.0 {
                value.min(lower_max) / lower_max
            } else {
                0.0
            };
            -fraction * mirror_lower_extent(upper_max)
        }
        (Baseline::Mirror { upper_max, .. }, SeriesDirection::Up) => value.min(upper_max),
        _ => value,
    }
}

impl SceneSeries<'_> {
    fn is_empty(&self) -> bool {
        match self.data {
            SeriesData::Dense(data) => data.is_empty(),
            SeriesData::Gapped(points) => points.iter().all(|point| point.y.is_none()),
        }
    }
}

fn fraction(numerator: usize, denominator: usize) -> f64 {
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    {
        numerator as f64 / denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;
    use crate::tui::widgets::hyperchart::model::ChartPoint;

    #[test]
    fn scene_empty_tracks_gap_only_series() {
        let points = [
            ChartPoint { x: 0.0, y: None },
            ChartPoint { x: 1.0, y: None },
        ];
        let scene = ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max: 1.0,
                y_min: 0.0,
                y_max: 1.0,
            },
            baseline: Baseline::Zero { y_max: 1.0 },
            series: vec![SceneSeries {
                name: "empty",
                data: SeriesData::Gapped(&points),
                line_color: Color::Reset,
                fill: FillStyle::None,
                direction: SeriesDirection::Up,
            }],
            grid: GridSpec { tick_count: 4 },
            annotations: Vec::new(),
        };

        assert!(scene.is_empty());
    }

    #[test]
    fn mirror_scene_transforms_downward_series() {
        let data = [(0.0, 42.0)];
        let scene = ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max: 1.0,
                y_min: -50.0,
                y_max: 100.0,
            },
            baseline: Baseline::Mirror {
                upper_max: 100.0,
                lower_max: 50.0,
                upper_label: "RX",
                lower_label: "TX",
            },
            series: vec![SceneSeries {
                name: "tx",
                data: SeriesData::Dense(&data),
                line_color: Color::Reset,
                fill: FillStyle::None,
                direction: SeriesDirection::Down,
            }],
            grid: GridSpec { tick_count: 4 },
            annotations: Vec::new(),
        };

        assert!((scene.transform_y(&scene.series[0], 75.0) + 50.0).abs() < f64::EPSILON);
        assert!((scene.transform_y(&scene.series[0], 25.0) + 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mirror_lower_lane_keeps_its_share_under_a_large_upper_scale() {
        let baseline = Baseline::Mirror {
            upper_max: 1_200.0,
            lower_max: 120.0,
            upper_label: "RX",
            lower_label: "TX",
        };

        // A full-scale lower value reaches half the upper lane's height no
        // matter how far apart the two scales are.
        let full = transform_y(baseline, SeriesDirection::Down, 120.0);
        let half = transform_y(baseline, SeriesDirection::Down, 60.0);
        assert!((full + 600.0).abs() < f64::EPSILON);
        assert!((half + 300.0).abs() < f64::EPSILON);
        assert!((mirror_lower_extent(1_200.0) - 600.0).abs() < f64::EPSILON);

        // A sub-unit lower scale still saturates at its own maximum.
        let tiny = Baseline::Mirror {
            upper_max: 1_200.0,
            lower_max: 0.5,
            upper_label: "RX",
            lower_label: "TX",
        };
        let above_max = transform_y(tiny, SeriesDirection::Down, 0.75);
        assert!((above_max + 600.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mirror_gridlines_include_both_halves() {
        let scene = ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max: 1.0,
                y_min: -30.0,
                y_max: 90.0,
            },
            baseline: Baseline::Mirror {
                upper_max: 90.0,
                lower_max: 30.0,
                upper_label: "RX",
                lower_label: "TX",
            },
            series: Vec::new(),
            grid: GridSpec { tick_count: 4 },
            annotations: Vec::new(),
        };

        assert_eq!(
            scene.gridlines(),
            vec![30.0, -15.0, 60.0, -30.0, 90.0, -45.0]
        );
    }
}
