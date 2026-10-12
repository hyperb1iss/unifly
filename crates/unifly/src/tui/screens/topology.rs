//! Topology screen - canvas-based network graph.

mod input;
mod layout;
mod render;
mod state;

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::Device;

use crate::tui::action::Action;
use crate::tui::component::Component;

pub struct TopologyScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
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
