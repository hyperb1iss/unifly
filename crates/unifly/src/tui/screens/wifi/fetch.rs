//! When the WiFi screen asks the controller for data, and what it does with
//! each answer. The observability reads need a session and the SSID
//! records need the Integration API, so nothing is asked for without the
//! access it needs, and a failure is kept for its panel instead of toasted.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::roam::parse_roam_rows;
use super::{ClientDetail, Failure, OverviewPanel, WifiScreen};
use crate::tui::action::{Action, WifiSubTab};

pub(super) const NEIGHBOR_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
pub(super) const CHANNEL_REFRESH_INTERVAL: Duration = Duration::from_mins(1);
/// How long a failed client fetch waits before it is tried again.
pub(super) const RETRY_INTERVAL: Duration = Duration::from_secs(15);
/// How old a client's WiFi detail gets before it is fetched again.
pub(super) const CLIENT_DETAIL_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
/// How long the client cursor rests before its detail is fetched, so
/// scrolling past a client does not ask for it.
pub(super) const CURSOR_SETTLE: Duration = Duration::from_millis(300);
/// Neighbors older than this many seconds are left out of the scan.
const NEIGHBOR_WINDOW_SECS: i64 = 300;
/// Roam events asked for per client.
const ROAM_HISTORY_LIMIT: u32 = 100;

fn due(last: Option<Instant>, interval: Duration, now: Instant) -> bool {
    last.is_none_or(|last| now.duration_since(last) >= interval)
}

pub(super) fn recent(failures: &HashMap<String, Failure>, key: &str, now: Instant) -> bool {
    failures
        .get(key)
        .is_some_and(|failure| now.duration_since(failure.at) < RETRY_INTERVAL)
}

pub(super) fn record(failures: &mut HashMap<String, Failure>, key: &str, error: &str) {
    let now = Instant::now();
    failures.retain(|_, failure| now.duration_since(failure.at) < RETRY_INTERVAL);
    failures.insert(
        key.to_owned(),
        Failure {
            at: now,
            error: error.to_owned(),
        },
    );
}

impl WifiScreen {
    /// Fetch the neighbor scan and channel list on the next tick that
    /// wants them.
    pub(super) fn mark_refresh_due(&mut self) {
        self.last_neighbors_request_at = None;
        self.last_channels_request_at = None;
    }

    pub(super) fn channel_map_open(&self) -> bool {
        self.sub_tab == WifiSubTab::Overview && self.overview_panel == OverviewPanel::Channels
    }

    /// The one request this tick sends, if anything on screen is stale.
    pub(super) fn tick_request(&mut self) -> Option<Action> {
        // Nothing is asked for until the controller has said what it can
        // reach, and each read waits for the access it needs.
        if !self.focused {
            return None;
        }
        let access = self.access?;
        let now = Instant::now();

        if access.integration
            && self.sub_tab == WifiSubTab::Ssids
            && let Some(request) = self.ssid_detail_request(now)
        {
            return Some(request);
        }
        if !access.session {
            return None;
        }

        if (self.sub_tab == WifiSubTab::Neighbors || self.channel_map_open())
            && due(
                self.last_neighbors_request_at,
                NEIGHBOR_REFRESH_INTERVAL,
                now,
            )
        {
            self.last_neighbors_request_at = Some(now);
            return Some(Action::RequestWifiNeighbors(Some(NEIGHBOR_WINDOW_SECS)));
        }

        if self.channel_map_open()
            && due(self.last_channels_request_at, CHANNEL_REFRESH_INTERVAL, now)
        {
            self.last_channels_request_at = Some(now);
            return Some(Action::RequestWifiChannels);
        }

        if self.sub_tab == WifiSubTab::Clients
            && self.client_detail_open
            && self
                .client_cursor_moved_at
                .is_none_or(|moved| now.duration_since(moved) >= CURSOR_SETTLE)
            && let Some(ip) = self.selected_client_ip()
            && !recent(&self.client_detail_failures, &ip, now)
            && let Some(request) = self.request_client_detail(ip, now)
        {
            return Some(request);
        }

        if self.sub_tab == WifiSubTab::Roaming
            && let Some(mac) = self.roam_target.as_ref().map(|target| target.mac.clone())
            && self.roam_history_mac.as_deref() != Some(mac.as_str())
            && !recent(&self.roam_failures, &mac, now)
        {
            return self.request_roam_history();
        }

        None
    }

    /// Ask for the WiFi detail of the client at `ip`, unless it is already
    /// on its way or what is on hand is fresh.
    pub(super) fn request_client_detail(&mut self, ip: String, now: Instant) -> Option<Action> {
        if self.client_detail_pending_ip.as_deref() == Some(ip.as_str()) {
            return None;
        }
        let fresh = self.client_detail.as_ref().is_some_and(|detail| {
            detail.ip == ip && now.duration_since(detail.loaded_at) < CLIENT_DETAIL_REFRESH_INTERVAL
        });
        if fresh {
            return None;
        }
        self.client_detail_pending_ip = Some(ip.clone());
        Some(Action::RequestWifiClientDetail(ip))
    }

    /// A response can land after the selection moved on: it settles its
    /// own request, and is kept only when its client is still selected.
    pub(super) fn client_detail_loaded(&mut self, ip: &String, data: &Arc<Value>) {
        if self.client_detail_pending_ip.as_ref() == Some(ip) {
            self.client_detail_pending_ip = None;
        }
        self.client_detail_failures.remove(ip);
        if self.selected_client_ip().as_ref() != Some(ip) {
            return;
        }
        self.client_detail = Some(ClientDetail {
            ip: ip.clone(),
            data: Arc::clone(data),
            loaded_at: Instant::now(),
        });
    }

    pub(super) fn client_detail_failed(&mut self, ip: &String, error: &str) {
        if self.client_detail_pending_ip.as_ref() == Some(ip) {
            self.client_detail_pending_ip = None;
        }
        record(&mut self.client_detail_failures, ip, error);
    }

    /// Why the detail of the client at `ip` could not be fetched.
    pub(super) fn client_detail_error(&self, ip: &str) -> Option<&str> {
        self.client_detail_failures
            .get(ip)
            .map(|failure| failure.error.as_str())
    }

    /// Ask for the roam history of the Roaming tab's client, unless it is
    /// already on its way.
    pub(super) fn request_roam_history(&mut self) -> Option<Action> {
        let mac = self.roam_target.as_ref()?.mac.clone();
        if self.roam_history_pending_mac.as_ref() == Some(&mac) {
            return None;
        }
        self.roam_history_pending_mac = Some(mac.clone());
        Some(Action::RequestWifiRoamHistory {
            mac,
            limit: Some(ROAM_HISTORY_LIMIT),
        })
    }

    /// The tab may have moved to another client while the fetch ran, and
    /// the history shown always belongs to the tab's client, so a response
    /// for another client only settles its own request.
    pub(super) fn roam_history_loaded(&mut self, mac: &String, events: &Arc<Vec<Value>>) {
        if self.roam_history_pending_mac.as_ref() == Some(mac) {
            self.roam_history_pending_mac = None;
        }
        if self.roam_target.as_ref().map(|target| &target.mac) != Some(mac) {
            return;
        }
        self.roam_failures.remove(mac);
        self.roam_history_mac = Some(mac.clone());
        self.roam_rows = parse_roam_rows(events);
        let last = self.roam_rows.len().checked_sub(1);
        let selected = self.roam_table_state.selected().unwrap_or(0);
        self.roam_table_state
            .select(last.map(|last| selected.min(last)));
    }

    pub(super) fn roam_history_failed(&mut self, mac: &String, error: &str) {
        if self.roam_history_pending_mac.as_ref() == Some(mac) {
            self.roam_history_pending_mac = None;
        }
        record(&mut self.roam_failures, mac, error);
    }

    /// Why the roam history of the Roaming tab's client could not be
    /// fetched.
    pub(super) fn roam_history_error(&self) -> Option<&str> {
        let target = self.roam_target.as_ref()?;
        self.roam_failures
            .get(&target.mac)
            .map(|failure| failure.error.as_str())
    }
}
