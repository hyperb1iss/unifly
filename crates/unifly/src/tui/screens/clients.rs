//! Clients screen - client table with type filters.

mod input;
mod labels;
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

use unifly_api::{Client, EntityId};

use crate::tui::action::{Action, ClientTypeFilter};
use crate::tui::component::Component;
use crate::tui::widgets::click::{Click, HitMap};

pub struct ClientsScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    clients: Arc<Vec<Arc<Client>>>,
    /// Device and network names for each client's uplink and network labels.
    links: labels::LinkIndex,
    table_state: TableState,
    filter: ClientTypeFilter,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    search_query: String,
    detail_open: bool,
    detail_client_id: Option<EntityId>,
}

impl Default for ClientsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ClientsScreen {
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

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Clients"
    }
}
