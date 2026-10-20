//! Topology screen - canvas-based network graph.

mod input;
mod layout;
mod render;
mod state;

use std::cell::RefCell;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::Device;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::widgets::click::{Click, HitMap};

pub struct TopologyScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
}

impl Default for TopologyScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TopologyScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        self.apply_action(action);
        Ok(None)
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
        "Topo"
    }
}
