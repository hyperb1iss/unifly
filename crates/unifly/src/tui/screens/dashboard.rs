//! Dashboard screen - top-level info density with Braille traffic graph.
//!
//! The heavy lifting lives in the `dashboard/` submodules so this file stays
//! focused on the public screen type and the `Component` glue.

use std::sync::Arc;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::{Client, Device, Event, HealthSummary, Network};

use crate::tui::action::Action;
use crate::tui::component::Component;
mod helpers;
mod render;
mod state;

/// How often the live traffic chart records a real sample. The controller
/// refreshes gateway throughput about once a second (WebSocket `device:sync`)
/// or every poll (API-key mode), so sampling faster only repeats values.
///
/// The chart is *drawn* on every 250 ms tick while the dashboard is focused:
/// between samples it scrolls by the fraction of the interval elapsed and
/// holds the last real value up to "now". The animation never invents data;
/// the ring holds one measurement per second.
pub(super) const LIVE_CHART_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
/// Samples kept in the ring: a one-minute window at 1 Hz.
pub(super) const LIVE_CHART_WINDOW_SAMPLES: usize = 60;
pub(super) const BANDWIDTH_TICK_COUNT: usize = 4;
pub(super) const BANDWIDTH_LABEL_WIDTH: usize = 6;
pub(super) const BANDWIDTH_GUTTER_WIDTH: u16 = 7;
pub(super) const MIN_BANDWIDTH_SCALE: f64 = 10_000.0;
pub(super) const BANDWIDTH_SCALE_WINDOW_SAMPLES: usize = 48;
pub(super) const BANDWIDTH_SCALE_PERCENTILE: usize = 85;

#[derive(Clone, Copy)]
pub(super) struct BandwidthSample {
    tx_bps: u64,
    rx_bps: u64,
    captured_at: Instant,
}

/// Dashboard screen state.
pub struct DashboardScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    networks: Arc<Vec<Arc<Network>>>,
    events: Vec<Arc<Event>>,
    health: Arc<Vec<HealthSummary>>,
    /// Chart data: `(sample_counter, bytes_per_sec)` for the Chart widget.
    bandwidth_tx: Vec<(f64, f64)>,
    bandwidth_rx: Vec<(f64, f64)>,
    /// Track peak rates for chart title.
    peak_tx: u64,
    peak_rx: u64,
    device_bandwidth: Option<BandwidthSample>,
    health_bandwidth: Option<BandwidthSample>,
    /// Monotonic sample counter - x-axis value.
    sample_counter: f64,
    chart_tx_y_max: f64,
    chart_rx_y_max: f64,
    last_chart_sample_at: Option<Instant>,
    /// Tracks when we last received a data update (for refresh indicator).
    last_data_update: Option<Instant>,
}

impl Default for DashboardScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            networks: Arc::new(Vec::new()),
            events: Vec::new(),
            health: Arc::new(Vec::new()),
            bandwidth_tx: Vec::new(),
            bandwidth_rx: Vec::new(),
            peak_tx: 0,
            peak_rx: 0,
            device_bandwidth: None,
            health_bandwidth: None,
            sample_counter: 0.0,
            chart_tx_y_max: 0.0,
            chart_rx_y_max: 0.0,
            last_chart_sample_at: None,
            last_data_update: None,
        }
    }
}

impl Component for DashboardScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<Action>> {
        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        Ok(self.apply_action(action))
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_dashboard(frame, area);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Dashboard"
    }
}
