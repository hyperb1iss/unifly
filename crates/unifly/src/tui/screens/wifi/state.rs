use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use ratatui::widgets::TableState;
use unifly_api::model::WifiBroadcast;
use unifly_api::session_models::RogueAp;
use unifly_api::{Client, ClientType, Device, DeviceType, EntityId, MacAddress};

use super::{FocusedAp, OverviewPanel, RoamTarget, WifiScreen};
use crate::cli::commands::util::order;
use crate::tui::action::{Action, WifiBand, WifiSortField, WifiSubTab};
use crate::tui::table::{TableScroll, reselect};

/// Identities of the selected table rows, see [`WifiScreen::selected_rows`].
pub(super) struct SelectedRows {
    ap: Option<EntityId>,
    client: Option<EntityId>,
    ssid: Option<EntityId>,
    neighbor: Option<String>,
}

impl WifiScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            access: None,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            ssids: Arc::new(Vec::new()),
            loaded: super::Loaded::default(),
            network_names: HashMap::new(),
            ap_names: HashMap::new(),
            ap_clients: super::client_index::ApClients::default(),
            ap_table_state: TableState::default(),
            client_table_state: TableState::default(),
            ssid_table_state: TableState::default(),
            neighbor_table_state: TableState::default(),
            roam_table_state: TableState::default(),
            ap_scroll: TableScroll::default(),
            client_scroll: TableScroll::default(),
            ssid_scroll: TableScroll::default(),
            neighbor_scroll: TableScroll::default(),
            roam_scroll: TableScroll::default(),
            sub_tab: WifiSubTab::default(),
            hits: std::cell::RefCell::default(),
            ap_sort: WifiSortField::Health,
            client_sort: WifiSortField::Health,
            neighbor_sort: WifiSortField::Signal,
            selected_band: WifiBand::default(),
            overview_panel: OverviewPanel::None,
            client_detail_open: false,
            ssid_detail_open: false,
            ap_query: String::new(),
            client_query: String::new(),
            ssid_query: String::new(),
            neighbor_query: String::new(),
            focused_ap: None,
            roam_target: None,
            neighbors: Arc::new(Vec::new()),
            neighbors_loaded_at: None,
            neighbors_error: None,
            channels: Arc::new(Vec::new()),
            channels_error: None,
            client_detail: None,
            client_detail_pending_ip: None,
            client_detail_failures: HashMap::new(),
            client_cursor_moved_at: None,
            roam_history_mac: None,
            roam_history_pending_mac: None,
            roam_rows: Vec::new(),
            roam_failures: HashMap::new(),
            last_neighbors_request_at: None,
            last_channels_request_at: None,
            ssid_prompt: None,
            ssid_details: super::ssid_detail::SsidDetails::default(),
        }
    }

    /// Forget everything that came from the controller. The sub-tab, sort
    /// orders, and band are the user's and stay.
    fn reset(&mut self) {
        let fresh = Self::new();
        *self = Self {
            focused: self.focused,
            action_tx: self.action_tx.take(),
            sub_tab: self.sub_tab,
            ap_sort: self.ap_sort,
            client_sort: self.client_sort,
            neighbor_sort: self.neighbor_sort,
            selected_band: self.selected_band,
            ..fresh
        };
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::Tick => return self.tick_request(),
            Action::AccessUpdated(access) => {
                self.access = Some(*access);
                if self.focused {
                    self.mark_refresh_due();
                }
            }
            Action::ControllerChanged => self.reset(),
            Action::WifiSubTab(sub_tab) => self.enter_sub_tab(*sub_tab),
            Action::WifiFocusAp(focused) => {
                let selected = self.selected_rows();
                self.focused_ap = focused.as_ref().and_then(|id| self.focus_for(id));
                self.follow_rows(&selected);
            }
            Action::WifiToggleChannelMap => {
                self.overview_panel = if self.overview_panel == OverviewPanel::Channels {
                    OverviewPanel::None
                } else {
                    OverviewPanel::Channels
                };
            }
            Action::WifiSortColumn(sort_field) => {
                let selected = self.selected_rows();
                match self.sub_tab {
                    WifiSubTab::Overview => self.ap_sort = *sort_field,
                    WifiSubTab::Clients => self.client_sort = *sort_field,
                    WifiSubTab::Neighbors => self.neighbor_sort = *sort_field,
                    WifiSubTab::Ssids | WifiSubTab::Roaming => {}
                }
                self.follow_rows(&selected);
            }
            Action::WifiBandSelect(band) => {
                self.selected_band = *band;
                self.sync_selections();
            }
            Action::SearchInput(query) => {
                if let Some(active) = self.active_query_mut() {
                    active.clone_from(query);
                    // A filter that just changed starts from its best match.
                    self.select_first();
                }
            }
            Action::CloseSearch => {
                // Clearing a filter stays on the row it found.
                let selected = self.selected_rows();
                if let Some(active) = self.active_query_mut() {
                    active.clear();
                }
                self.follow_rows(&selected);
            }
            Action::PromptSubmitted(text) => return self.ssid_prompt_submitted(text),
            data => self.apply_data(data),
        }

        None
    }

    /// Take in what the controller sent: entity snapshots and the answers
    /// to this screen's own fetches.
    fn apply_data(&mut self, action: &Action) {
        match action {
            Action::DevicesUpdated(devices) => {
                let selected = self.selected_rows();
                self.devices = Arc::new(order::devices(devices));
                self.loaded.devices = true;
                self.ap_names = self
                    .devices
                    .iter()
                    .map(|device| (device.mac.clone(), display_device_name(device)))
                    .collect();
                self.refresh_focused_ap();
                self.follow_rows(&selected);
            }
            Action::ClientsUpdated(clients) => {
                // Client health reorders the AP table too, so the AP
                // selection follows its AP on every client snapshot,
                // including the first; restart and locate act on it.
                let selected = self.selected_rows();
                self.clients = Arc::new(order::clients(clients));
                self.loaded.clients = true;
                self.ap_clients = super::client_index::ApClients::new(&self.clients);
                self.follow_rows(&selected);
            }
            Action::NetworksUpdated(networks) => {
                self.network_names = networks
                    .iter()
                    .map(|network| (network.id.clone(), network.name.clone()))
                    .collect();
            }
            Action::WifiBroadcastsUpdated(ssids) => {
                let selected = self.selected_rows();
                let mut sorted: Vec<Arc<WifiBroadcast>> = ssids.to_vec();
                sorted.sort_by_cached_key(|ssid| (ssid.name.to_lowercase(), ssid.id.to_string()));
                self.ssids = Arc::new(sorted);
                self.loaded.ssids = true;
                self.ssid_details.follow(&self.ssids);
                self.follow_rows(&selected);
            }
            Action::WifiBroadcastDetailLoaded { id, detail } => {
                self.ssid_detail_loaded(id, detail);
            }
            Action::WifiBroadcastDetailFailed { id, error } => {
                self.ssid_detail_failed(id, error);
            }
            Action::WifiSsidWriteSettled { id, detail } => {
                self.ssid_write_settled(id, detail.as_ref());
            }
            Action::WifiNeighborsUpdated(neighbors) => {
                let selected = self.selected_rows();
                self.neighbors = Arc::clone(neighbors);
                self.neighbors_loaded_at = Some(std::time::Instant::now());
                self.neighbors_error = None;
                self.follow_rows(&selected);
            }
            Action::WifiNeighborsFailed(error) => {
                self.neighbors_error = Some(error.clone());
            }
            Action::WifiChannelsUpdated(channels) => {
                self.channels = Arc::clone(channels);
                self.channels_error = None;
            }
            Action::WifiChannelsFailed(error) => {
                self.channels_error = Some(error.clone());
            }
            Action::WifiClientDetailLoaded { ip, data } => self.client_detail_loaded(ip, data),
            Action::WifiClientDetailFailed { ip, error } => self.client_detail_failed(ip, error),
            Action::WifiRoamHistoryLoaded { mac, events } => self.roam_history_loaded(mac, events),
            Action::WifiRoamHistoryFailed { mac, error } => self.roam_history_failed(mac, error),
            _ => {}
        }
    }

    /// Switch to `sub_tab`. The Roaming tab shows the client selected on
    /// the Clients tab; with none selected it keeps the client it had.
    pub(super) fn enter_sub_tab(&mut self, sub_tab: WifiSubTab) {
        self.sub_tab = sub_tab;
        if sub_tab == WifiSubTab::Roaming {
            self.target_selected_client();
        }
        self.mark_refresh_due();
    }

    /// Point the Roaming tab at the client selected on the Clients tab.
    pub(super) fn target_selected_client(&mut self) {
        let Some(target) = self.selected_client().map(|client| RoamTarget {
            mac: client.mac.to_string(),
            label: display_client_name(client),
        }) else {
            return;
        };
        if self.roam_target.as_ref() != Some(&target) {
            if self.roam_target.as_ref().map(|old| &old.mac) != Some(&target.mac) {
                self.roam_rows.clear();
                self.roam_history_mac = None;
                self.roam_table_state.select(None);
            }
            self.roam_target = Some(target);
        }
    }

    /// The search filter of the active sub-tab, or `None` on Roaming,
    /// whose history search does not filter.
    pub(super) fn active_query(&self) -> Option<&str> {
        match self.sub_tab {
            WifiSubTab::Overview => Some(&self.ap_query),
            WifiSubTab::Clients => Some(&self.client_query),
            WifiSubTab::Ssids => Some(&self.ssid_query),
            WifiSubTab::Neighbors => Some(&self.neighbor_query),
            WifiSubTab::Roaming => None,
        }
    }

    fn active_query_mut(&mut self) -> Option<&mut String> {
        match self.sub_tab {
            WifiSubTab::Overview => Some(&mut self.ap_query),
            WifiSubTab::Clients => Some(&mut self.client_query),
            WifiSubTab::Ssids => Some(&mut self.ssid_query),
            WifiSubTab::Neighbors => Some(&mut self.neighbor_query),
            WifiSubTab::Roaming => None,
        }
    }

    /// Whether the controller is known to have no session access, which
    /// every observability read here needs.
    pub(super) fn session_missing(&self) -> bool {
        self.access.is_some_and(|access| !access.session)
    }

    /// Whether the controller is known to have no Integration access,
    /// which the SSID list and its writes need.
    pub(super) fn integration_missing(&self) -> bool {
        self.access.is_some_and(|access| !access.integration)
    }

    /// Every access point on the site, in name order.
    pub(super) fn all_aps(&self) -> impl Iterator<Item = &Arc<Device>> {
        self.devices
            .iter()
            .filter(|device| device.device_type == DeviceType::AccessPoint)
    }

    /// The AP table's rows: the access points the filter keeps, sorted.
    pub(super) fn ap_devices(&self) -> Vec<&Arc<Device>> {
        let query = self.ap_query.to_lowercase();
        let mut aps: Vec<_> = self
            .all_aps()
            .filter(|device| query.is_empty() || ap_matches(device, &query))
            .collect();

        // `self.devices` is already in name order and the sorts are stable,
        // so ties (and the Name sort) keep that order.
        match self.ap_sort {
            WifiSortField::Health => aps.sort_by_cached_key(|ap| Reverse(self.ap_health(ap))),
            WifiSortField::Clients => {
                aps.sort_by_cached_key(|ap| Reverse(self.ap_clients(ap).len()));
            }
            WifiSortField::Channel => aps.sort_by_cached_key(|ap| {
                let channel = self.ap_channel_profile(ap).map(|(channel, _)| channel);
                (channel.is_none(), channel)
            }),
            _ => {}
        }

        aps
    }

    /// The client table's rows: wireless clients on the focused AP that
    /// the filter keeps, sorted.
    pub(super) fn wireless_clients(&self) -> Vec<&Arc<Client>> {
        let query = self.client_query.to_lowercase();
        let focus = self.focused_ap.as_ref().map(|focused| &focused.mac);
        let mut clients: Vec<_> = self
            .clients
            .iter()
            .filter(|client| client.client_type == ClientType::Wireless)
            .filter(|client| client.wireless.is_some())
            .filter(|client| focus.is_none_or(|mac| client_on_ap(client, mac)))
            .filter(|client| query.is_empty() || client_matches(client, &query))
            .collect();

        // `self.clients` is already in name order and the sorts are stable,
        // so ties (and the Name sort) keep that order.
        match self.client_sort {
            WifiSortField::Health => clients.sort_by_key(|client| {
                Reverse(client.wireless.as_ref().and_then(|w| w.satisfaction))
            }),
            WifiSortField::Signal => clients
                .sort_by_key(|client| Reverse(client.wireless.as_ref().and_then(|w| w.signal_dbm))),
            _ => {}
        }

        clients
    }

    /// The SSID table's rows: the broadcasts the filter keeps, by name.
    pub(super) fn visible_ssids(&self) -> Vec<&Arc<WifiBroadcast>> {
        let query = self.ssid_query.to_lowercase();
        self.ssids
            .iter()
            .filter(|ssid| query.is_empty() || ssid.name.to_lowercase().contains(&query))
            .collect()
    }

    /// The neighbor table's rows: neighbors on the selected band, seen by
    /// the focused AP, that the filter keeps, sorted.
    pub(super) fn visible_neighbors(&self) -> Vec<&RogueAp> {
        let query = self.neighbor_query.to_lowercase();
        let mut neighbors: Vec<_> = self
            .neighbors
            .iter()
            .filter(|neighbor| self.neighbor_seen_by_focus(neighbor))
            .filter(|neighbor| {
                band_from_neighbor(neighbor).is_none_or(|band| band == self.selected_band)
            })
            .filter(|neighbor| query.is_empty() || neighbor_matches(neighbor, &query))
            .collect();

        // Hidden networks have no name to sort by and follow the named.
        let name = |neighbor: &RogueAp| {
            let essid = neighbor.essid.as_deref().filter(|essid| !essid.is_empty());
            (
                essid.is_none(),
                essid.unwrap_or(&neighbor.bssid).to_lowercase(),
            )
        };
        match self.neighbor_sort {
            WifiSortField::Channel => neighbors.sort_by_cached_key(|neighbor| {
                (neighbor.channel.unwrap_or(i32::MAX), name(neighbor))
            }),
            WifiSortField::Security => neighbors.sort_by_cached_key(|neighbor| {
                let security = neighbor.security.as_deref().unwrap_or("").to_lowercase();
                (security.is_empty(), security, name(neighbor))
            }),
            WifiSortField::Name => neighbors.sort_by_cached_key(|neighbor| name(neighbor)),
            _ => neighbors.sort_by_cached_key(|neighbor| {
                (Reverse(neighbor.signal.or(neighbor.rssi)), name(neighbor))
            }),
        }

        neighbors
    }

    pub(super) fn selected_ap(&self) -> Option<&Arc<Device>> {
        let aps = self.ap_devices();
        let index = self.ap_table_state.selected().unwrap_or(0);
        aps.get(index).copied()
    }

    pub(super) fn selected_client(&self) -> Option<&Arc<Client>> {
        let clients = self.wireless_clients();
        let index = self.client_table_state.selected().unwrap_or(0);
        clients.get(index).copied()
    }

    pub(super) fn selected_ssid(&self) -> Option<&Arc<WifiBroadcast>> {
        let ssids = self.visible_ssids();
        let index = self.ssid_table_state.selected().unwrap_or(0);
        ssids.get(index).copied()
    }

    fn selected_neighbor(&self) -> Option<&RogueAp> {
        let neighbors = self.visible_neighbors();
        let index = self.neighbor_table_state.selected().unwrap_or(0);
        neighbors.get(index).copied()
    }

    /// Identities of the rows selected in each table, taken before their
    /// rows change.
    pub(super) fn selected_rows(&self) -> SelectedRows {
        SelectedRows {
            ap: self.selected_ap().map(|device| device.id.clone()),
            client: self.selected_client().map(|client| client.id.clone()),
            ssid: self.selected_ssid().map(|ssid| ssid.id.clone()),
            neighbor: self.selected_neighbor().map(neighbor_key),
        }
    }

    /// Point each table back at the rows captured by `selected_rows`, so a
    /// refresh, re-sort, or filter change keeps the cursor on the same AP,
    /// client, SSID, and neighbor. A row that left gives its place to the
    /// row now at its index.
    pub(super) fn follow_rows(&mut self, selected: &SelectedRows) {
        let ap = reselect(
            self.ap_devices().into_iter().map(|device| &device.id),
            selected.ap.as_ref(),
            self.ap_table_state.selected(),
        );
        let client = reselect(
            self.wireless_clients().into_iter().map(|client| &client.id),
            selected.client.as_ref(),
            self.client_table_state.selected(),
        );
        let ssid = reselect(
            self.visible_ssids().into_iter().map(|ssid| &ssid.id),
            selected.ssid.as_ref(),
            self.ssid_table_state.selected(),
        );
        let neighbor_keys: Vec<String> = self
            .visible_neighbors()
            .into_iter()
            .map(neighbor_key)
            .collect();
        let neighbor = reselect(
            neighbor_keys.iter(),
            selected.neighbor.as_ref(),
            self.neighbor_table_state.selected(),
        );
        self.ap_table_state.select(ap);
        self.client_table_state.select(client);
        self.ssid_table_state.select(ssid);
        self.neighbor_table_state.select(neighbor);
    }

    /// Clamp every table's cursor to its rows.
    pub(super) fn sync_selections(&mut self) {
        let lens = [
            self.ap_devices().len(),
            self.wireless_clients().len(),
            self.visible_ssids().len(),
            self.visible_neighbors().len(),
            self.roam_rows.len(),
        ];
        let states = [
            &mut self.ap_table_state,
            &mut self.client_table_state,
            &mut self.ssid_table_state,
            &mut self.neighbor_table_state,
            &mut self.roam_table_state,
        ];
        for (state, len) in states.into_iter().zip(lens) {
            sync_selection(state, len);
        }
    }

    pub(super) fn selected_client_ip(&self) -> Option<String> {
        self.selected_client()
            .and_then(|client| client.ip.map(|ip| ip.to_string()))
    }

    /// The focus on the device `id`, when it is still an access point.
    fn focus_for(&self, id: &EntityId) -> Option<FocusedAp> {
        self.all_aps()
            .find(|device| &device.id == id)
            .map(|device| FocusedAp {
                id: device.id.clone(),
                mac: device.mac.clone(),
                label: display_device_name(device),
            })
    }

    /// Keep the focus on its AP across a device refresh, and drop it only
    /// when that AP has left the site.
    fn refresh_focused_ap(&mut self) {
        if let Some(focused) = &self.focused_ap {
            self.focused_ap = self.focus_for(&focused.id);
        }
    }

    pub(super) fn ap_clients(&self, ap: &Device) -> &[Arc<Client>] {
        self.ap_clients.get(&ap.mac)
    }

    pub(super) fn ap_health(&self, ap: &Device) -> Option<u8> {
        let (sum, count) = self
            .ap_clients(ap)
            .iter()
            .filter_map(|client| client.wireless.as_ref()?.satisfaction)
            .fold((0u32, 0u32), |(sum, count), score| {
                (sum + u32::from(score), count + 1)
            });
        (count > 0)
            .then(|| u8::try_from(sum / count).ok())
            .flatten()
    }

    pub(super) fn ap_signal(&self, ap: &Device) -> Option<i32> {
        let (sum, count) = self
            .ap_clients(ap)
            .iter()
            .filter_map(|client| client.wireless.as_ref()?.signal_dbm)
            .fold((0i32, 0i32), |(sum, count), dbm| (sum + dbm, count + 1));
        (count > 0).then(|| sum / count)
    }

    /// The name of the AP `client` is on, or a dash.
    pub(super) fn client_ap_name(&self, client: &Client) -> String {
        let bssid = client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.bssid.as_ref());
        client
            .uplink_device_mac
            .as_ref()
            .into_iter()
            .chain(bssid)
            .find_map(|mac| self.ap_names.get(mac))
            .cloned()
            .unwrap_or_else(|| "─".to_string())
    }

    /// How many wireless clients are on the SSID named `name`, or `None`
    /// without the session that says which SSID a client is on.
    pub(super) fn ssid_client_count(&self, name: &str) -> Option<usize> {
        if self.session_missing() {
            return None;
        }
        let on_ssid = |client: &&Arc<Client>| {
            client
                .wireless
                .as_ref()
                .and_then(|wireless| wireless.ssid.as_deref())
                == Some(name)
        };
        Some(self.clients.iter().filter(on_ssid).count())
    }

    /// How many clients are on `ap`, or `None` without the session that
    /// says which AP a client is on.
    pub(super) fn ap_client_count(&self, ap: &Device) -> Option<usize> {
        if self.session_missing() {
            return ap
                .client_count
                .and_then(|count| usize::try_from(count).ok());
        }
        Some(self.ap_clients(ap).len())
    }

    /// Whether `neighbor` was seen by the focused AP, or no AP is focused.
    pub(super) fn neighbor_seen_by_focus(&self, neighbor: &RogueAp) -> bool {
        self.focused_ap.as_ref().is_none_or(|focused| {
            neighbor
                .ap_mac
                .as_deref()
                .is_some_and(|mac| mac.eq_ignore_ascii_case(focused.mac.as_str()))
        })
    }

    /// The name of the AP that saw `neighbor`, or a dash.
    pub(super) fn neighbor_observer(&self, neighbor: &RogueAp) -> String {
        neighbor
            .ap_mac
            .as_deref()
            .and_then(|mac| self.ap_names.get(&MacAddress::new(mac)))
            .cloned()
            .unwrap_or_else(|| "─".to_string())
    }

    pub(super) fn select_first(&mut self) {
        if self.active_len() > 0 {
            self.select_row(0);
        }
    }

    /// Rows in the active sub-tab's table.
    pub(super) fn active_len(&self) -> usize {
        match self.sub_tab {
            WifiSubTab::Overview => self.ap_devices().len(),
            WifiSubTab::Clients => self.wireless_clients().len(),
            WifiSubTab::Ssids => self.visible_ssids().len(),
            WifiSubTab::Neighbors => self.visible_neighbors().len(),
            WifiSubTab::Roaming => self.roam_rows.len(),
        }
    }

    pub(super) fn active_table_state(&mut self) -> &mut TableState {
        match self.sub_tab {
            WifiSubTab::Overview => &mut self.ap_table_state,
            WifiSubTab::Clients => &mut self.client_table_state,
            WifiSubTab::Ssids => &mut self.ssid_table_state,
            WifiSubTab::Neighbors => &mut self.neighbor_table_state,
            WifiSubTab::Roaming => &mut self.roam_table_state,
        }
    }

    /// Move the active table's cursor to `index`.
    pub(super) fn select_row(&mut self, index: usize) {
        if self.active_table_state().selected() == Some(index) {
            return;
        }
        self.active_table_state().select(Some(index));
        if self.sub_tab == WifiSubTab::Clients {
            self.client_cursor_moved_at = Some(std::time::Instant::now());
        }
    }
}

fn sync_selection(table_state: &mut TableState, len: usize) {
    if len == 0 {
        table_state.select(None);
        return;
    }

    let next = table_state.selected().unwrap_or(0).min(len - 1);
    table_state.select(Some(next));
}

/// A neighbor row's identity. The controller lists a neighbor once per AP
/// that hears it, so the BSSID alone would name several rows.
fn neighbor_key(neighbor: &RogueAp) -> String {
    format!(
        "{}@{}",
        neighbor.bssid,
        neighbor.ap_mac.as_deref().unwrap_or_default()
    )
}

fn client_on_ap(client: &Client, ap: &MacAddress) -> bool {
    client.uplink_device_mac.as_ref() == Some(ap)
        || client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.bssid.as_ref())
            == Some(ap)
}

/// Whether an AP matches a lowercased search `query`.
fn ap_matches(device: &Device, query: &str) -> bool {
    display_device_name(device).to_lowercase().contains(query)
        || device.mac.as_str().contains(query)
        || device
            .radios
            .iter()
            .filter_map(|radio| radio.channel)
            .any(|channel| channel.to_string() == query)
}

/// Whether a client matches a lowercased search `query`.
fn client_matches(client: &Client, query: &str) -> bool {
    display_client_name(client).to_lowercase().contains(query)
        || client.addresses().any(|ip| ip.to_string().contains(query))
        || client.mac.as_str().contains(query)
        || client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.ssid.as_deref())
            .is_some_and(|ssid| ssid.to_lowercase().contains(query))
}

/// Whether a neighbor matches a lowercased search `query`.
fn neighbor_matches(neighbor: &RogueAp, query: &str) -> bool {
    neighbor
        .essid
        .as_deref()
        .is_some_and(|essid| essid.to_lowercase().contains(query))
        || neighbor.bssid.to_lowercase().contains(query)
        || neighbor
            .channel
            .is_some_and(|channel| channel.to_string() == query)
}

pub(super) fn display_device_name(device: &Device) -> String {
    device
        .name
        .clone()
        .unwrap_or_else(|| device.mac.to_string())
}

pub(super) fn display_client_name(client: &Client) -> String {
    client
        .name
        .clone()
        .or(client.hostname.clone())
        .unwrap_or_else(|| client.mac.to_string())
}

/// A neighbor's network name, or `None` when it hides its SSID.
pub(super) fn neighbor_name(neighbor: &RogueAp) -> Option<&str> {
    neighbor.essid.as_deref().filter(|essid| !essid.is_empty())
}

pub(super) fn band_from_frequency(frequency_ghz: f32) -> WifiBand {
    if frequency_ghz >= 5.9 {
        WifiBand::SixGhz
    } else if frequency_ghz >= 4.9 {
        WifiBand::FiveGhz
    } else {
        WifiBand::TwoGhz
    }
}

pub(super) fn band_from_channel(channel: u32) -> WifiBand {
    if channel >= 191 {
        WifiBand::SixGhz
    } else if channel > 14 {
        WifiBand::FiveGhz
    } else {
        WifiBand::TwoGhz
    }
}

pub(super) fn band_from_neighbor(neighbor: &RogueAp) -> Option<WifiBand> {
    neighbor.radio.as_deref().map_or_else(
        || {
            neighbor.freq.map(|freq| {
                if freq >= 5900 {
                    WifiBand::SixGhz
                } else if freq >= 4900 {
                    WifiBand::FiveGhz
                } else {
                    WifiBand::TwoGhz
                }
            })
        },
        |radio| match radio {
            "6e" | "6g" => Some(WifiBand::SixGhz),
            "na" => Some(WifiBand::FiveGhz),
            _ => Some(WifiBand::TwoGhz),
        },
    )
}
