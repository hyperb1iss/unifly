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
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::session_models::{ChannelAvailability, RogueAp};
use unifly_api::{Client, Device, EntityId};

use crate::tui::action::{Action, WifiBand, WifiSortField, WifiSubTab};
use crate::tui::component::Component;
use crate::tui::widgets::click::{Click, HitMap};

pub struct WifiScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    ap_table_state: TableState,
    client_table_state: TableState,
    neighbor_table_state: TableState,
    roam_table_state: TableState,
    sub_tab: WifiSubTab,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    sort_field: WifiSortField,
    selected_band: WifiBand,
    detail_open: bool,
    channel_map_open: bool,
    search_query: String,
    focused_ap_id: Option<EntityId>,
    focused_client_id: Option<EntityId>,
    neighbors: Arc<Vec<RogueAp>>,
    channels: Arc<Vec<ChannelAvailability>>,
    client_detail_ip: Option<String>,
    client_detail_pending_ip: Option<String>,
    client_detail: Option<Arc<Value>>,
    /// The client whose detail fetch failed last, and when, so the retry
    /// waits instead of firing on every tick.
    client_detail_failure: Option<(String, std::time::Instant)>,
    roam_history_mac: Option<String>,
    roam_history_pending_mac: Option<String>,
    roam_history: Arc<Vec<Value>>,
    last_neighbors_request_at: Option<std::time::Instant>,
    last_channels_request_at: Option<std::time::Instant>,
}

impl Default for WifiScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for WifiScreen {
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
        if focused {
            self.mark_refresh_due();
        }
    }

    fn search_query(&self) -> Option<&str> {
        Some(&self.search_query)
    }

    fn id(&self) -> &'static str {
        "WiFi"
    }
}
