//! Why a WiFi table has no rows: still loading, nothing there, nothing
//! matching, or out of reach for this profile. Each table draws the one
//! line that applies instead of a bare header.

use crate::tui::screens::wifi::WifiScreen;

pub(super) const NO_SESSION_CHANNELS: &str =
    "The channel map needs a direct controller session. Cloud profiles cannot read it.";
pub(super) const NO_SESSION_EXPERIENCE: &str =
    "WiFi experience needs a direct controller session. Cloud profiles cannot read it.";
const NO_SESSION_CLIENTS: &str =
    "Wireless client data needs a direct controller session. Cloud profiles cannot read it.";
const NO_SESSION_NEIGHBORS: &str =
    "Neighbor scans need a direct controller session. Cloud profiles cannot read them.";
const NO_SESSION_ROAMS: &str =
    "Roam history needs a direct controller session. Cloud profiles cannot read it.";
const NO_INTEGRATION_SSIDS: &str = "SSIDs come from the Integration API. A profile that signs in with a password only cannot read them.";

/// " matching "query"", or nothing without a filter.
fn matching(query: &str) -> String {
    if query.is_empty() {
        String::new()
    } else {
        format!(" matching \"{query}\"")
    }
}

impl WifiScreen {
    pub(super) fn ap_empty_reason(&self) -> String {
        if !self.loaded.devices {
            "Loading access points\u{2026}".to_owned()
        } else if self.all_aps().next().is_none() {
            "No access points on this site".to_owned()
        } else {
            format!("No access point{}", matching(&self.ap_query))
        }
    }

    pub(super) fn client_empty_reason(&self) -> String {
        if self.session_missing() {
            return NO_SESSION_CLIENTS.to_owned();
        }
        if !self.loaded.clients {
            return "Loading wireless clients\u{2026}".to_owned();
        }
        let on_ap = self
            .focused_ap
            .as_ref()
            .map_or_else(String::new, |focused| format!(" on {}", focused.label));
        format!("No wireless clients{on_ap}{}", matching(&self.client_query))
    }

    pub(super) fn ssid_empty_reason(&self) -> String {
        if self.integration_missing() {
            NO_INTEGRATION_SSIDS.to_owned()
        } else if !self.loaded.ssids {
            "Loading SSIDs\u{2026}".to_owned()
        } else {
            format!("No SSIDs{}", matching(&self.ssid_query))
        }
    }

    pub(super) fn neighbor_empty_reason(&self) -> String {
        if self.session_missing() {
            return NO_SESSION_NEIGHBORS.to_owned();
        }
        if self.neighbors_loaded_at.is_none() {
            return match &self.neighbors_error {
                Some(error) => format!("Neighbor scan unavailable: {error}"),
                None => "Reading the neighbor scan\u{2026}".to_owned(),
            };
        }
        let by_ap = self
            .focused_ap
            .as_ref()
            .map_or_else(String::new, |focused| format!(" by {}", focused.label));
        format!(
            "No neighbors heard on {}{by_ap}{}",
            self.selected_band.label(),
            matching(&self.neighbor_query)
        )
    }

    /// Why the Roaming table is empty, or `None` when it has rows.
    pub(super) fn roam_empty_reason(&self) -> Option<String> {
        if self.session_missing() {
            return Some(NO_SESSION_ROAMS.to_owned());
        }
        let Some(target) = &self.roam_target else {
            return Some(
                "Pick a client on the Clients tab and press r to see where it has roamed."
                    .to_owned(),
            );
        };
        if !self.roam_rows.is_empty() {
            return None;
        }
        let label = &target.label;
        Some(if let Some(error) = self.roam_history_error() {
            format!("Roam history unavailable: {error}. Retrying.")
        } else if self.roam_history_mac.as_ref() != Some(&target.mac) {
            format!("Loading roam history for {label}\u{2026}")
        } else {
            format!("No roam events recorded for {label}")
        })
    }
}
