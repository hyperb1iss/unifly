//! Stats screen - historical charts and analytics.

mod input;
mod render;
mod state;

use std::cell::RefCell;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::action::{Action, StatsPeriod};
use crate::tui::component::Component;
use crate::tui::widgets::click::{Click, HitMap};

pub struct StatsScreen {
    focused: bool,
    period: StatsPeriod,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    bandwidth_tx: Vec<(f64, f64)>,
    bandwidth_rx: Vec<(f64, f64)>,
    bandwidth_tx_y_max: f64,
    bandwidth_rx_y_max: f64,
    client_counts: Vec<(f64, f64)>,
    client_y_max: f64,
    bandwidth_peak: f64,
    dpi_apps: Vec<(String, u64)>,
    dpi_categories: Vec<(String, u64)>,
}

const BANDWIDTH_TICK_COUNT: usize = 4;
const BANDWIDTH_LABEL_WIDTH: usize = 6;
const MIN_BANDWIDTH_SCALE: f64 = 10_000.0;
const CLIENT_TICK_COUNT: usize = 4;
const CLIENT_LABEL_WIDTH: usize = 5;

impl Default for StatsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for StatsScreen {
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        Ok(self.apply_action(action))
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Stats"
    }
}
