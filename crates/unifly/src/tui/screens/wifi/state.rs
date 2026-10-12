use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::widgets::TableState;
use serde_json::Value;
use unifly_api::session_models::RogueAp;
use unifly_api::{Client, ClientType, Device, DeviceType, EntityId};

use super::WifiScreen;
use crate::cli::commands::util::order;
use crate::tui::action::{Action, WifiBand, WifiSortField, WifiSubTab};
use crate::tui::table::reselect;

pub(super) const NEIGHBOR_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
pub(super) const CHANNEL_REFRESH_INTERVAL: Duration = Duration::from_mins(1);

#[derive(Clone, Default)]
pub(super) struct ChannelOccupancy {
    pub channel: i32,
    pub your_count: usize,
    pub neighbor_count: usize,
    pub signal: Option<i32>,
    pub conflict: bool,
}

/// Identities of the selected table rows, see [`WifiScreen::selected_rows`].
pub(super) struct SelectedRows {
    ap: Option<EntityId>,
    client: Option<EntityId>,
    neighbor: Option<String>,
}

#[derive(Clone)]
pub(super) struct ParsedRoamRow {
    pub timestamp: String,
    pub event: String,
    pub from_ap: String,
    pub to_ap: String,
    pub signal: String,
    pub band: String,
}

impl WifiScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            ap_table_state: TableState::default(),
            client_table_state: TableState::default(),
            neighbor_table_state: TableState::default(),
            roam_table_state: TableState::default(),
            sub_tab: WifiSubTab::default(),
            sort_field: WifiSortField::default(),
            selected_band: WifiBand::default(),
            detail_open: false,
            channel_map_open: false,
            search_query: String::new(),
            focused_ap_id: None,
            focused_client_id: None,
            neighbors: Arc::new(Vec::new()),
            channels: Arc::new(Vec::new()),
            client_detail_ip: None,
            client_detail_pending_ip: None,
            client_detail: None,
            roam_history_mac: None,
            roam_history_pending_mac: None,
            roam_history: Arc::new(Vec::new()),
            last_neighbors_request_at: None,
            last_channels_request_at: None,
        }
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::Tick => return self.tick_request(),
            Action::DevicesUpdated(devices) => {
                let selected = self.selected_rows();
                self.devices = Arc::new(order::devices(devices));
                self.follow_rows(&selected);
                self.sync_ap_selection();
                self.sync_client_selection();
                self.sync_neighbor_selection();
            }
            Action::ClientsUpdated(clients) => {
                // Client health reorders the AP table too, so the AP
                // selection follows its AP on every client snapshot,
                // including the first; restart and locate act on it.
                let selected = self.selected_rows();
                self.clients = Arc::new(order::clients(clients));
                self.follow_rows(&selected);
                self.sync_client_selection();
            }
            Action::WifiNeighborsUpdated(neighbors) => {
                let selected = self.selected_rows();
                self.neighbors = Arc::clone(neighbors);
                self.follow_rows(&selected);
                self.sync_neighbor_selection();
            }
            Action::WifiChannelsUpdated(channels) => {
                self.channels = Arc::clone(channels);
            }
            Action::WifiClientDetailLoaded { ip, data } => {
                self.client_detail_ip = Some(ip.clone());
                self.client_detail_pending_ip = None;
                self.client_detail = Some(Arc::clone(data));
            }
            Action::WifiRoamHistoryLoaded { mac, events } => {
                self.roam_history_mac = Some(mac.clone());
                self.roam_history_pending_mac = None;
                self.roam_history = Arc::clone(events);
                self.sync_roam_selection();
            }
            Action::WifiSubTab(sub_tab) => {
                self.sub_tab = *sub_tab;
                if !matches!(self.sub_tab, WifiSubTab::Overview | WifiSubTab::Clients) {
                    self.detail_open = false;
                }
                if self.sub_tab == WifiSubTab::Roaming {
                    self.focus_client_from_selection();
                }
            }
            Action::WifiFocusAp(focused) => {
                self.focused_ap_id.clone_from(focused);
                self.sync_client_selection();
                self.sync_neighbor_selection();
            }
            Action::WifiToggleChannelMap => {
                self.channel_map_open = !self.channel_map_open;
            }
            Action::WifiSortColumn(sort_field) => {
                let selected = self.selected_rows();
                self.sort_field = *sort_field;
                self.follow_rows(&selected);
                self.sync_ap_selection();
                self.sync_client_selection();
                self.sync_neighbor_selection();
            }
            Action::WifiBandSelect(band) => {
                self.selected_band = *band;
                self.sync_neighbor_selection();
            }
            Action::SearchInput(query) => {
                self.search_query.clone_from(query);
                self.sync_ap_selection();
                self.sync_client_selection();
                self.sync_neighbor_selection();
            }
            Action::CloseSearch => {
                self.search_query.clear();
                self.sync_ap_selection();
                self.sync_client_selection();
                self.sync_neighbor_selection();
            }
            Action::CloseDetail => {
                self.detail_open = false;
            }
            _ => {}
        }

        None
    }

    pub(super) fn mark_refresh_due(&mut self) {
        let now = Instant::now();
        self.last_neighbors_request_at = now.checked_sub(NEIGHBOR_REFRESH_INTERVAL);
        self.last_channels_request_at = now.checked_sub(CHANNEL_REFRESH_INTERVAL);
    }

    fn tick_request(&mut self) -> Option<Action> {
        if !self.focused {
            return None;
        }

        let now = Instant::now();

        if matches!(self.sub_tab, WifiSubTab::Overview | WifiSubTab::Neighbors)
            && self
                .last_neighbors_request_at
                .is_none_or(|last| now.duration_since(last) >= NEIGHBOR_REFRESH_INTERVAL)
        {
            self.last_neighbors_request_at = Some(now);
            return Some(Action::RequestWifiNeighbors(Some(300)));
        }

        if self.channel_map_open
            && self
                .last_channels_request_at
                .is_none_or(|last| now.duration_since(last) >= CHANNEL_REFRESH_INTERVAL)
        {
            self.last_channels_request_at = Some(now);
            return Some(Action::RequestWifiChannels);
        }

        if self.sub_tab == WifiSubTab::Clients
            && self.detail_open
            && let Some(ip) = self.selected_client_ip()
            && self.client_detail_ip.as_deref() != Some(ip.as_str())
            && self.client_detail_pending_ip.as_deref() != Some(ip.as_str())
        {
            self.client_detail_pending_ip = Some(ip.clone());
            return Some(Action::RequestWifiClientDetail(ip));
        }

        if self.sub_tab == WifiSubTab::Roaming
            && let Some(mac) = self.focused_client_mac()
            && self.roam_history_mac.as_deref() != Some(mac.as_str())
            && self.roam_history_pending_mac.as_deref() != Some(mac.as_str())
        {
            self.roam_history_pending_mac = Some(mac.clone());
            return Some(Action::RequestWifiRoamHistory {
                mac,
                limit: Some(100),
            });
        }

        None
    }

    pub(super) fn ap_devices(&self) -> Vec<&Arc<Device>> {
        let mut aps: Vec<_> = self
            .devices
            .iter()
            .filter(|device| device.device_type == DeviceType::AccessPoint)
            .filter(|device| self.matches_ap_query(device))
            .collect();

        // `self.devices` is already in name order and the sort is stable, so
        // ties (and the Name sort) keep that order.
        aps.sort_by(|left, right| match self.sort_field {
            WifiSortField::Health => self.ap_health(right).cmp(&self.ap_health(left)),
            WifiSortField::Clients => self
                .ap_clients(right)
                .len()
                .cmp(&self.ap_clients(left).len()),
            WifiSortField::Channel => self
                .ap_channel_profile(left)
                .map(|(channel, _)| channel)
                .unwrap_or_default()
                .cmp(
                    &self
                        .ap_channel_profile(right)
                        .map(|(channel, _)| channel)
                        .unwrap_or_default(),
                ),
            _ => Ordering::Equal,
        });

        aps
    }

    pub(super) fn wireless_clients(&self) -> Vec<&Arc<Client>> {
        let mut clients: Vec<_> = self
            .clients
            .iter()
            .filter(|client| client.client_type == ClientType::Wireless)
            .filter(|client| client.wireless.is_some())
            .filter(|client| self.client_matches_focused_ap(client))
            .filter(|client| self.matches_client_query(client))
            .collect();

        // `self.clients` is already in name order and the sort is stable, so
        // ties (and the Name sort) keep that order.
        clients.sort_by(|left, right| match self.sort_field {
            WifiSortField::Health => right
                .wireless
                .as_ref()
                .and_then(|wireless| wireless.satisfaction)
                .cmp(
                    &left
                        .wireless
                        .as_ref()
                        .and_then(|wireless| wireless.satisfaction),
                ),
            WifiSortField::Signal => right
                .wireless
                .as_ref()
                .and_then(|wireless| wireless.signal_dbm)
                .cmp(
                    &left
                        .wireless
                        .as_ref()
                        .and_then(|wireless| wireless.signal_dbm),
                ),
            _ => Ordering::Equal,
        });

        clients
    }

    pub(super) fn visible_neighbors(&self) -> Vec<&RogueAp> {
        let mut neighbors: Vec<_> = self
            .neighbors
            .iter()
            .filter(|neighbor| self.neighbor_matches_focused_ap(neighbor))
            .filter(|neighbor| self.neighbor_matches_selected_band(neighbor))
            .filter(|neighbor| self.matches_neighbor_query(neighbor))
            .collect();

        neighbors.sort_by(|left, right| match self.sort_field {
            WifiSortField::Channel => left
                .channel
                .unwrap_or_default()
                .cmp(&right.channel.unwrap_or_default())
                .then_with(|| display_neighbor_name(left).cmp(&display_neighbor_name(right))),
            WifiSortField::Security => left
                .security
                .as_deref()
                .unwrap_or("")
                .cmp(right.security.as_deref().unwrap_or(""))
                .then_with(|| display_neighbor_name(left).cmp(&display_neighbor_name(right))),
            _ => right
                .signal
                .or(right.rssi)
                .cmp(&left.signal.or(left.rssi))
                .then_with(|| display_neighbor_name(left).cmp(&display_neighbor_name(right))),
        });

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

    pub(super) fn selected_neighbor(&self) -> Option<&RogueAp> {
        let neighbors = self.visible_neighbors();
        let index = self.neighbor_table_state.selected().unwrap_or(0);
        neighbors.get(index).copied()
    }

    /// Identities of the rows selected in the AP, client, and neighbor
    /// tables, taken before their rows change.
    pub(super) fn selected_rows(&self) -> SelectedRows {
        SelectedRows {
            ap: self.selected_ap().map(|device| device.id.clone()),
            client: self.selected_client().map(|client| client.id.clone()),
            neighbor: self
                .selected_neighbor()
                .map(|neighbor| neighbor.bssid.clone()),
        }
    }

    /// Point each table back at the rows captured by `selected_rows`, so a
    /// refresh or re-sort keeps the cursor on the same AP, client, and
    /// neighbor.
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
        let neighbor = reselect(
            self.visible_neighbors()
                .into_iter()
                .map(|neighbor| neighbor.bssid.as_str()),
            selected.neighbor.as_deref(),
            self.neighbor_table_state.selected(),
        );
        self.ap_table_state.select(ap);
        self.client_table_state.select(client);
        self.neighbor_table_state.select(neighbor);
    }

    pub(super) fn selected_client_ip(&self) -> Option<String> {
        self.selected_client()
            .and_then(|client| client.ip.map(|ip| ip.to_string()))
    }

    pub(super) fn focused_ap_label(&self) -> Option<String> {
        self.focused_ap().map(|device| display_device_name(device))
    }

    pub(super) fn focused_client_label(&self) -> Option<String> {
        self.clients
            .iter()
            .find(|client| Some(&client.id) == self.focused_client_id.as_ref())
            .map(|client| display_client_name(client))
    }

    pub(super) fn focused_client_mac(&self) -> Option<String> {
        self.clients
            .iter()
            .find(|client| Some(&client.id) == self.focused_client_id.as_ref())
            .map(|client| client.mac.to_string())
    }

    pub(super) fn ap_clients(&self, ap: &Device) -> Vec<&Arc<Client>> {
        self.clients
            .iter()
            .filter(|client| client.client_type == ClientType::Wireless)
            .filter(|client| {
                client
                    .uplink_device_mac
                    .as_ref()
                    .is_some_and(|mac| mac == &ap.mac)
                    || client
                        .wireless
                        .as_ref()
                        .and_then(|wireless| wireless.bssid.as_ref())
                        .is_some_and(|bssid| bssid == &ap.mac)
            })
            .collect()
    }

    pub(super) fn ap_health(&self, ap: &Device) -> Option<u8> {
        let satisfaction: Vec<u32> = self
            .ap_clients(ap)
            .iter()
            .filter_map(|client| client.wireless.as_ref()?.satisfaction)
            .map(u32::from)
            .collect();

        if satisfaction.is_empty() {
            return None;
        }

        let count = u32::try_from(satisfaction.len()).unwrap_or(1);
        u8::try_from(satisfaction.iter().sum::<u32>() / count).ok()
    }

    pub(super) fn ap_signal(&self, ap: &Device) -> Option<i32> {
        let signals: Vec<i32> = self
            .ap_clients(ap)
            .iter()
            .filter_map(|client| client.wireless.as_ref()?.signal_dbm)
            .collect();

        if signals.is_empty() {
            return None;
        }

        let count = i32::try_from(signals.len()).unwrap_or(1);
        Some(signals.iter().sum::<i32>() / count)
    }

    pub(super) fn client_ap_name(&self, client: &Client) -> String {
        let ap = self.devices.iter().find(|device| {
            client
                .uplink_device_mac
                .as_ref()
                .is_some_and(|mac| mac == &device.mac)
                || client
                    .wireless
                    .as_ref()
                    .and_then(|wireless| wireless.bssid.as_ref())
                    .is_some_and(|bssid| bssid == &device.mac)
        });

        ap.map_or_else(|| "─".to_string(), |device| display_device_name(device))
    }

    pub(super) fn channel_band_slice(&self) -> Vec<i32> {
        let mut channels = match self.selected_band {
            WifiBand::TwoGhz => self
                .channels
                .first()
                .and_then(|entry| entry.channels_ng.clone())
                .unwrap_or_else(|| vec![1, 6, 11]),
            WifiBand::FiveGhz => self
                .channels
                .first()
                .and_then(|entry| entry.channels_na.clone())
                .unwrap_or_default(),
            WifiBand::SixGhz => self
                .channels
                .first()
                .and_then(|entry| entry.channels_6e.clone())
                .unwrap_or_default(),
        };

        if self.selected_band != WifiBand::TwoGhz {
            let mut active = BTreeSet::new();
            for device in self.ap_devices() {
                if let Some((channel, band)) = self.ap_channel_profile(device)
                    && band == self.selected_band
                    && let Ok(channel) = i32::try_from(channel)
                {
                    active.insert(channel);
                }
            }
            for neighbor in self
                .neighbors
                .iter()
                .filter(|neighbor| self.neighbor_matches_focused_ap(neighbor))
            {
                if band_from_neighbor(neighbor) == Some(self.selected_band)
                    && let Some(channel) = neighbor.channel
                {
                    active.insert(channel);
                }
            }
            if !active.is_empty() {
                channels.retain(|channel| active.contains(channel));
                if channels.is_empty() {
                    channels = active.into_iter().collect();
                }
            }
        }

        channels
    }

    pub(super) fn channel_occupancy(&self) -> Vec<ChannelOccupancy> {
        self.channel_band_slice()
            .into_iter()
            .map(|channel| {
                let your_count = self
                    .ap_devices()
                    .iter()
                    .filter_map(|device| self.ap_channel_profile(device))
                    .filter(|(ap_channel, band)| {
                        *band == self.selected_band
                            && i32::try_from(*ap_channel).ok() == Some(channel)
                    })
                    .count();
                let neighbor_count = self
                    .neighbors
                    .iter()
                    .filter(|neighbor| self.neighbor_matches_focused_ap(neighbor))
                    .filter(|neighbor| band_from_neighbor(neighbor) == Some(self.selected_band))
                    .filter(|neighbor| neighbor.channel == Some(channel))
                    .count();
                let signal = self
                    .neighbors
                    .iter()
                    .filter(|neighbor| self.neighbor_matches_focused_ap(neighbor))
                    .filter(|neighbor| band_from_neighbor(neighbor) == Some(self.selected_band))
                    .filter(|neighbor| neighbor.channel == Some(channel))
                    .filter_map(|neighbor| neighbor.signal.or(neighbor.rssi))
                    .max();

                ChannelOccupancy {
                    channel,
                    your_count,
                    neighbor_count,
                    signal,
                    conflict: your_count > 0 && neighbor_count > 0,
                }
            })
            .collect()
    }

    pub(super) fn parsed_roam_rows(&self) -> Vec<ParsedRoamRow> {
        let mut rows: Vec<_> = self.roam_history.iter().map(parse_roam_row).collect();
        rows.sort_by(|left, right| compare_timestamp(&right.timestamp, &left.timestamp));
        rows
    }

    fn focused_ap(&self) -> Option<&Arc<Device>> {
        self.devices
            .iter()
            .find(|device| Some(&device.id) == self.focused_ap_id.as_ref())
    }

    fn matches_ap_query(&self, device: &Device) -> bool {
        if self.search_query.is_empty() {
            return true;
        }

        let query = self.search_query.to_ascii_lowercase();
        display_device_name(device)
            .to_ascii_lowercase()
            .contains(&query)
            || device.mac.to_string().contains(&query)
            || device
                .radios
                .iter()
                .filter_map(|radio| radio.channel)
                .any(|channel| channel.to_string().contains(&query))
    }

    fn matches_client_query(&self, client: &Client) -> bool {
        if self.search_query.is_empty() {
            return true;
        }

        let query = self.search_query.to_ascii_lowercase();
        display_client_name(client)
            .to_ascii_lowercase()
            .contains(&query)
            || client
                .ip
                .map(|ip| ip.to_string())
                .unwrap_or_default()
                .contains(&query)
            || client.mac.to_string().contains(&query)
    }

    fn matches_neighbor_query(&self, neighbor: &RogueAp) -> bool {
        if self.search_query.is_empty() {
            return true;
        }

        let query = self.search_query.to_ascii_lowercase();
        display_neighbor_name(neighbor)
            .to_ascii_lowercase()
            .contains(&query)
            || neighbor.bssid.to_ascii_lowercase().contains(&query)
            || neighbor
                .channel
                .is_some_and(|channel| channel.to_string().contains(&query))
    }

    fn client_matches_focused_ap(&self, client: &Client) -> bool {
        let Some(focused) = self.focused_ap() else {
            return true;
        };

        client
            .uplink_device_mac
            .as_ref()
            .is_some_and(|mac| mac == &focused.mac)
            || client
                .wireless
                .as_ref()
                .and_then(|wireless| wireless.bssid.as_ref())
                .is_some_and(|bssid| bssid == &focused.mac)
    }

    fn neighbor_matches_focused_ap(&self, neighbor: &RogueAp) -> bool {
        let Some(focused) = self.focused_ap() else {
            return true;
        };
        neighbor.ap_mac.as_deref() == Some(focused.mac.as_str())
    }

    fn neighbor_matches_selected_band(&self, neighbor: &RogueAp) -> bool {
        band_from_neighbor(neighbor).is_none_or(|band| band == self.selected_band)
    }

    pub(super) fn ap_channel_profile(&self, ap: &Device) -> Option<(u32, WifiBand)> {
        if let Some((channel, band)) = ap
            .radios
            .iter()
            .filter_map(|radio| {
                radio
                    .channel
                    .map(|channel| (channel, band_from_frequency(radio.frequency_ghz)))
            })
            .min_by_key(|(channel, _)| *channel)
        {
            return Some((channel, band));
        }

        let mut counts = BTreeMap::new();
        for client in self.ap_clients(ap) {
            let Some(wireless) = client.wireless.as_ref() else {
                continue;
            };
            let Some(channel) = wireless.channel else {
                continue;
            };
            let band = wireless
                .frequency_ghz
                .map_or_else(|| band_from_channel(channel), band_from_frequency);
            let key = (channel, band_index(band));
            counts
                .entry(key)
                .and_modify(|count| *count += 1usize)
                .or_insert(1usize);
        }

        counts
            .into_iter()
            .max_by(|(left_key, left_count), (right_key, right_count)| {
                left_count
                    .cmp(right_count)
                    .then_with(|| right_key.0.cmp(&left_key.0))
            })
            .map(|((channel, band), _)| (channel, band_from_index(band)))
    }

    pub(super) fn sync_ap_selection(&mut self) {
        let len = self.ap_devices().len();
        sync_selection(&mut self.ap_table_state, len);
        if self
            .focused_ap_id
            .as_ref()
            .is_some_and(|focused| !self.ap_devices().iter().any(|device| &device.id == focused))
        {
            self.focused_ap_id = None;
        }
    }

    pub(super) fn sync_client_selection(&mut self) {
        let len = self.wireless_clients().len();
        sync_selection(&mut self.client_table_state, len);
        if self.focused_client_id.as_ref().is_some_and(|focused| {
            !self
                .wireless_clients()
                .iter()
                .any(|client| &client.id == focused)
        }) {
            self.focused_client_id = None;
            self.roam_history = Arc::new(Vec::new());
            self.roam_history_mac = None;
            self.roam_history_pending_mac = None;
        }
    }

    pub(super) fn sync_neighbor_selection(&mut self) {
        let len = self.visible_neighbors().len();
        sync_selection(&mut self.neighbor_table_state, len);
    }

    pub(super) fn sync_roam_selection(&mut self) {
        let len = self.roam_history.len();
        sync_selection(&mut self.roam_table_state, len);
    }

    pub(super) fn focus_client_from_selection(&mut self) {
        let selected = self.selected_client().map(|client| client.id.clone());
        if selected != self.focused_client_id {
            self.focused_client_id = selected;
            self.roam_history = Arc::new(Vec::new());
            self.roam_history_mac = None;
            self.roam_history_pending_mac = None;
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

pub(super) fn display_neighbor_name(neighbor: &RogueAp) -> String {
    neighbor
        .essid
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| neighbor.bssid.clone())
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

pub(super) fn parse_roam_row(event: &Value) -> ParsedRoamRow {
    let timestamp_value = event
        .get("timestamp")
        .or_else(|| event.get("time"))
        .and_then(Value::as_i64);
    let timestamp = timestamp_value.map_or_else(|| "─".to_string(), format_timestamp);
    let parameters = event.get("parameters").unwrap_or(event);

    ParsedRoamRow {
        timestamp,
        event: json_string(event, &["event_type", "event"])
            .unwrap_or_else(|| "unknown".to_string()),
        from_ap: nested_parameter(parameters, &["DEVICE_FROM", "from_ap"])
            .unwrap_or_else(|| "─".to_string()),
        to_ap: nested_parameter(parameters, &["DEVICE_TO", "to_ap", "ap_mac"])
            .unwrap_or_else(|| "─".to_string()),
        signal: nested_parameter(parameters, &["SIGNAL_STRENGTH", "signal"])
            .map_or_else(|| "─".to_string(), |value| format!("{value} dBm")),
        band: nested_parameter(parameters, &["RADIO_BAND", "band"])
            .unwrap_or_else(|| "─".to_string()),
    }
}

pub(super) fn json_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(found) = value.get(key) {
            if let Some(text) = found.as_str() {
                return Some(text.to_string());
            }
            if let Some(number) = found.as_i64() {
                return Some(number.to_string());
            }
            if let Some(number) = found.as_u64() {
                return Some(number.to_string());
            }
        }
    }
    None
}

pub(super) fn json_i32(value: &Value, keys: &[&str]) -> Option<i32> {
    for key in keys {
        if let Some(found) = value.get(key).and_then(Value::as_i64)
            && let Ok(found) = i32::try_from(found)
        {
            return Some(found);
        }
    }
    None
}

pub(super) fn json_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(found) = value.get(key).and_then(Value::as_u64) {
            return Some(found);
        }
    }
    None
}

fn nested_parameter(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(found) = value.get(key) else {
            continue;
        };
        if let Some(name) = found.get("name").and_then(Value::as_str) {
            return Some(name.to_string());
        }
        if let Some(name) = found.as_str() {
            return Some(name.to_string());
        }
        if let Some(number) = found.as_i64() {
            return Some(number.to_string());
        }
    }
    None
}

fn band_index(band: WifiBand) -> u8 {
    match band {
        WifiBand::TwoGhz => 0,
        WifiBand::FiveGhz => 1,
        WifiBand::SixGhz => 2,
    }
}

fn band_from_index(index: u8) -> WifiBand {
    match index {
        1 => WifiBand::FiveGhz,
        2 => WifiBand::SixGhz,
        _ => WifiBand::TwoGhz,
    }
}

fn format_timestamp(timestamp: i64) -> String {
    if timestamp > 1_000_000_000_000 {
        chrono::DateTime::from_timestamp_millis(timestamp).map_or_else(
            || timestamp.to_string(),
            |value| value.format("%H:%M:%S").to_string(),
        )
    } else {
        chrono::DateTime::from_timestamp(timestamp, 0).map_or_else(
            || timestamp.to_string(),
            |value| value.format("%H:%M:%S").to_string(),
        )
    }
}

fn compare_timestamp(left: &str, right: &str) -> Ordering {
    left.cmp(right)
}

#[cfg(test)]
mod tests;
