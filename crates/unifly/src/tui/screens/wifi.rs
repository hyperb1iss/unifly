mod channels;
mod client_index;
mod fetch;
mod input;
mod keys;
mod render;
mod roam;
mod ssid_detail;
mod ssids;
mod state;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::model::WifiBroadcast;
use unifly_api::session_models::{ChannelAvailability, RogueAp};
use unifly_api::{Client, Device, EntityId, MacAddress};

use crate::tui::action::{Access, Action, WifiBand, WifiSortField, WifiSubTab};
use crate::tui::component::Component;
use crate::tui::table::TableScroll;
use crate::tui::widgets::click::{Click, HitMap};

/// What the Overview draws beside the AP table. One panel at a time, so
/// nothing waits hidden behind another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum OverviewPanel {
    #[default]
    None,
    Detail,
    Channels,
}

/// The AP the Clients and Neighbors tables are narrowed to.
#[derive(Debug, Clone)]
struct FocusedAp {
    id: EntityId,
    mac: MacAddress,
    label: String,
}

/// The client the Roaming tab shows. It is kept by MAC and label so the
/// history stays up after the client leaves the list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RoamTarget {
    mac: String,
    label: String,
}

/// The WiFi detail on hand, and whose it is.
struct ClientDetail {
    ip: String,
    data: Arc<Value>,
    loaded_at: Instant,
}

#[derive(Debug, Clone, Copy, Default)]
struct Loaded {
    devices: bool,
    clients: bool,
    ssids: bool,
}

/// A fetch that failed: when, for the retry delay, and why, for the panel.
struct Failure {
    at: Instant,
    error: String,
}

pub struct WifiScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    /// What the controller can reach; `None` until it has connected.
    access: Option<Access>,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    ssids: Arc<Vec<Arc<WifiBroadcast>>>,
    /// Which of those the controller has sent at least once, to tell an
    /// empty site from one still loading.
    loaded: Loaded,
    network_names: HashMap<EntityId, String>,
    /// AP names by MAC, for the client and neighbor tables.
    ap_names: HashMap<MacAddress, String>,
    ap_clients: client_index::ApClients,
    ap_table_state: TableState,
    client_table_state: TableState,
    ssid_table_state: TableState,
    neighbor_table_state: TableState,
    roam_table_state: TableState,
    ap_scroll: TableScroll,
    client_scroll: TableScroll,
    ssid_scroll: TableScroll,
    neighbor_scroll: TableScroll,
    roam_scroll: TableScroll,
    sub_tab: WifiSubTab,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    ap_sort: WifiSortField,
    client_sort: WifiSortField,
    neighbor_sort: WifiSortField,
    selected_band: WifiBand,
    overview_panel: OverviewPanel,
    client_detail_open: bool,
    ssid_detail_open: bool,
    ap_query: String,
    client_query: String,
    ssid_query: String,
    neighbor_query: String,
    focused_ap: Option<FocusedAp>,
    roam_target: Option<RoamTarget>,
    neighbors: Arc<Vec<RogueAp>>,
    /// When the neighbor scan last arrived; `None` until the first one.
    neighbors_loaded_at: Option<Instant>,
    neighbors_error: Option<String>,
    channels: Arc<Vec<ChannelAvailability>>,
    channels_error: Option<String>,
    client_detail: Option<ClientDetail>,
    client_detail_pending_ip: Option<String>,
    /// Failed detail fetches by client IP.
    client_detail_failures: HashMap<String, Failure>,
    /// When the client cursor last moved, so a scroll through the list
    /// fetches only the detail it settles on.
    client_cursor_moved_at: Option<Instant>,
    /// The client whose history `roam_rows` holds.
    roam_history_mac: Option<String>,
    roam_history_pending_mac: Option<String>,
    roam_rows: Vec<roam::ParsedRoamRow>,
    /// Failed roam history fetches by client MAC.
    roam_failures: HashMap<String, Failure>,
    last_neighbors_request_at: Option<Instant>,
    last_channels_request_at: Option<Instant>,
    /// What the open prompt was asked for.
    ssid_prompt: Option<ssids::SsidPrompt>,
    /// The SSIDs' full records, which the list leaves most of out.
    ssid_details: ssid_detail::SsidDetails,
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
        } else {
            // Nothing typed into a prompt outlives the screen's turn.
            self.ssid_prompt = None;
        }
    }

    fn search_query(&self) -> Option<&str> {
        self.active_query()
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        self.help_keys()
    }

    fn id(&self) -> &'static str {
        "WiFi"
    }
}
