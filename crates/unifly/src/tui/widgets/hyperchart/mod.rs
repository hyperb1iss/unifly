//! HyperChart — unified time-series and ranked-bar widgets for the unifly TUI.
//!
//! Two widgets live here:
//!
//! - [`HyperChart`] for time-series visualisations (WAN bandwidth, client
//!   counts, anything with an x/y axis). Two back-ends: [`Renderer::Canvas`]
//!   for hero panels (Octant marker, manual gutter) and [`Renderer::Tiled`]
//!   for dense grid cells (Ratatui `Chart` widget, built-in axes).
//! - [`HyperBars`] for ranked horizontal bar lists (top apps, traffic
//!   categories). Denominator is configurable (max-observed or total).
//!
//! Both widgets share axis math ([`axis`]), empty-state rendering
//! ([`empty`]), and block styling ([`block`]), so any visual refinement
//! lands in one place.

mod annotations;
pub mod axis;
pub mod bars;
pub mod block;
mod canvas;
pub mod color;
#[cfg(feature = "tui-graphics")]
mod curve;
pub mod empty;
pub mod faceplate;
mod geometry;
pub mod heatmap;
pub mod model;
#[cfg(feature = "tui-graphics")]
mod owned_scene;
#[cfg(feature = "tui-graphics")]
mod pixel;
#[cfg(feature = "tui-graphics")]
pub mod raster;
pub mod scene;
mod subcell;
mod tiled;
pub mod time_series;

pub use bars::{Denominator, HyperBars, Row, ValueFormat};
pub use color::ChartGradient;
pub use faceplate::SwitchFaceplate;
pub use heatmap::{HeatmapCell, HyperHeatmap};
pub use model::{
    Baseline, ChartPoint, Domain, FillStyle, Series, SeriesData, SeriesDirection, XAxis,
};
pub use scene::{Annotation, AnnotationKind, ChartScene, GridSpec, PlotBounds, SceneSeries};
pub use time_series::{HyperChart, Renderer};
