//! Deep-owned [`ChartScene`] snapshot for cross-thread rasterization.

use ratatui::style::Color;

use super::model::{Baseline, ChartPoint, FillStyle, SeriesData, SeriesDirection, XAxis};
use super::scene::{Annotation, ChartScene, GridSpec, PlotBounds, SceneSeries};

#[derive(Debug, Clone)]
enum OwnedSeriesData {
    Dense(Vec<(f64, f64)>),
    Gapped(Vec<ChartPoint>),
}

#[derive(Debug, Clone)]
struct OwnedSeries {
    name: String,
    data: OwnedSeriesData,
    line_color: Color,
    fill: FillStyle,
    direction: SeriesDirection,
}

#[derive(Debug, Clone)]
enum OwnedBaseline {
    Zero {
        y_max: f64,
    },
    Mirror {
        upper_max: f64,
        lower_max: f64,
        upper_label: String,
        lower_label: String,
    },
}

/// Deep-owned snapshot of a [`ChartScene`], detached from screen-state
/// borrows so it can cross into the graphics worker thread.
#[derive(Debug, Clone)]
pub(super) struct OwnedChartScene {
    x_axis: XAxis,
    bounds: PlotBounds,
    baseline: OwnedBaseline,
    series: Vec<OwnedSeries>,
    grid: GridSpec,
    annotations: Vec<Annotation>,
}

impl OwnedChartScene {
    pub(super) fn capture(scene: &ChartScene<'_>) -> Self {
        Self {
            x_axis: scene.x_axis,
            bounds: scene.bounds,
            baseline: match scene.baseline {
                Baseline::Zero { y_max } => OwnedBaseline::Zero { y_max },
                Baseline::Mirror {
                    upper_max,
                    lower_max,
                    upper_label,
                    lower_label,
                } => OwnedBaseline::Mirror {
                    upper_max,
                    lower_max,
                    upper_label: upper_label.to_string(),
                    lower_label: lower_label.to_string(),
                },
            },
            series: scene
                .series
                .iter()
                .map(|series| OwnedSeries {
                    name: series.name.to_string(),
                    data: match series.data {
                        SeriesData::Dense(points) => OwnedSeriesData::Dense(points.to_vec()),
                        SeriesData::Gapped(points) => OwnedSeriesData::Gapped(points.to_vec()),
                    },
                    line_color: series.line_color,
                    fill: series.fill,
                    direction: series.direction,
                })
                .collect(),
            grid: scene.grid,
            annotations: scene.annotations.clone(),
        }
    }

    pub(super) fn as_scene(&self) -> ChartScene<'_> {
        ChartScene {
            x_axis: self.x_axis,
            bounds: self.bounds,
            baseline: match &self.baseline {
                OwnedBaseline::Zero { y_max } => Baseline::Zero { y_max: *y_max },
                OwnedBaseline::Mirror {
                    upper_max,
                    lower_max,
                    upper_label,
                    lower_label,
                } => Baseline::Mirror {
                    upper_max: *upper_max,
                    lower_max: *lower_max,
                    upper_label,
                    lower_label,
                },
            },
            series: self
                .series
                .iter()
                .map(|series| SceneSeries {
                    name: &series.name,
                    data: match &series.data {
                        OwnedSeriesData::Dense(points) => SeriesData::Dense(points),
                        OwnedSeriesData::Gapped(points) => SeriesData::Gapped(points),
                    },
                    line_color: series.line_color,
                    fill: series.fill,
                    direction: series.direction,
                })
                .collect(),
            grid: self.grid,
            annotations: self.annotations.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_round_trips_through_as_scene() {
        let data = [(0.0, 1.0), (1.0, 3.0)];
        let gapped = [
            ChartPoint {
                x: 0.0,
                y: Some(2.0),
            },
            ChartPoint { x: 1.0, y: None },
        ];
        let scene = ChartScene {
            x_axis: XAxis::Hidden,
            bounds: PlotBounds {
                x_min: 0.0,
                x_max: 1.0,
                y_min: 0.0,
                y_max: 4.0,
            },
            baseline: Baseline::Mirror {
                upper_max: 4.0,
                lower_max: 2.0,
                upper_label: "RX",
                lower_label: "TX",
            },
            series: vec![
                SceneSeries {
                    name: "rx",
                    data: SeriesData::Dense(&data),
                    line_color: Color::Cyan,
                    fill: FillStyle::Solid(Color::Blue),
                    direction: SeriesDirection::Up,
                },
                SceneSeries {
                    name: "tx",
                    data: SeriesData::Gapped(&gapped),
                    line_color: Color::Magenta,
                    fill: FillStyle::None,
                    direction: SeriesDirection::Down,
                },
            ],
            grid: GridSpec { tick_count: 4 },
            annotations: Vec::new(),
        };

        let owned = OwnedChartScene::capture(&scene);
        let view = owned.as_scene();

        assert_eq!(view.bounds, scene.bounds);
        assert_eq!(view.series.len(), 2);
        assert_eq!(view.series[0].name, "rx");
        match view.series[0].data {
            SeriesData::Dense(points) => assert_eq!(points, &data),
            SeriesData::Gapped(_) => panic!("dense series became gapped"),
        }
        match view.series[1].data {
            SeriesData::Gapped(points) => assert_eq!(points, &gapped),
            SeriesData::Dense(_) => panic!("gapped series became dense"),
        }
        match view.baseline {
            Baseline::Mirror {
                upper_label,
                lower_label,
                ..
            } => {
                assert_eq!(upper_label, "RX");
                assert_eq!(lower_label, "TX");
            }
            Baseline::Zero { .. } => panic!("mirror baseline became zero"),
        }
    }
}
