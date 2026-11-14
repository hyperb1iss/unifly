//! Clients screen: the client table with its type filter and sort, and a
//! detail pane for the selected client.

mod commands;
mod input;
mod labels;
mod render;
mod state;
mod view;

use std::cell::RefCell;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use unifly_api::{Client, EntityId};

use crate::tui::action::{Access, Action, ClientTypeFilter};
use crate::tui::component::{Component, LIST_HELP};
use crate::tui::table::TableScroll;
use crate::tui::widgets::click::{Click, HitMap};

pub struct ClientsScreen {
    focused: bool,
    /// Every connected client, by name.
    clients: Arc<Vec<Arc<Client>>>,
    /// Device and network names for each client's uplink and network labels.
    links: labels::LinkIndex,
    /// What the controller can reach. `None` until it has connected.
    access: Option<Access>,
    table_state: TableState,
    table_scroll: TableScroll,
    filter: ClientTypeFilter,
    sort: view::ClientSort,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    search_query: String,
    detail_open: bool,
    detail_client_id: Option<EntityId>,
    /// What the open prompt's answer is for.
    asking: Option<commands::Asking>,
    /// Blocked clients that are off the network, which the controller
    /// lists apart from the connected ones.
    offline_blocked: Arc<Vec<Arc<Client>>>,
    offline_blocked_status: state::Fetch,
    /// The Blocked filter's rows: connected and offline, by name.
    blocked_rows: Vec<Arc<Client>>,
}

impl Default for ClientsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ClientsScreen {
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

    fn search_query(&self) -> Option<&str> {
        Some(&self.search_query)
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = Vec::new();
        if self.detail_open {
            keys.extend([
                ("j/k \u{2191}/\u{2193}", "Previous / next client"),
                ("g/G", "First / last client"),
                ("h/l \u{2190}/\u{2192}", "Filter (closes detail)"),
            ]);
        } else {
            keys.extend(LIST_HELP);
            keys.extend([
                ("Enter", "Open the client"),
                ("h/l \u{2190}/\u{2192}", "Previous / next filter"),
                ("s", "Sort by the next column"),
                ("r", "Reverse the sort"),
            ]);
        }
        keys.extend(CLIENT_KEYS);
        if self.detail_open {
            keys.push(("Esc", "Close the detail"));
        }
        keys
    }

    fn id(&self) -> &'static str {
        "Clients"
    }
}

/// Commands for the selected client, in the table and in the detail.
const CLIENT_KEYS: &[(&str, &str)] = &[
    ("b", "Block / unblock"),
    ("x", "Kick (wireless only)"),
    ("I", "Set / remove fixed IP"),
    ("A", "Authorize / revoke guest"),
    ("F", "Forget the client"),
];
