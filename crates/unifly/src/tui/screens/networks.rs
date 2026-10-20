//! Networks screen - network table with inline detail expansion and editing overlay.

mod input;
mod render;
mod state;

use std::cell::RefCell;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::Network;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::table::TableScroll;
use crate::tui::widgets::click::{Click, HitMap};

use self::state::NetworkEditState;

pub struct NetworksScreen {
    focused: bool,
    networks: Arc<Vec<Arc<Network>>>,
    table_state: TableState,
    table_scroll: TableScroll,
    detail_open: bool,
    edit_state: Option<NetworkEditState>,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    action_tx: Option<UnboundedSender<Action>>,
}

impl Default for NetworksScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for NetworksScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
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

    fn captures_input(&self) -> bool {
        self.edit_state.is_some()
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Networks"
    }
}
